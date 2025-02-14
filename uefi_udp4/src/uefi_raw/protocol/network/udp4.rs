use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::ops::Deref;
use core::ptr::NonNull;
use log::warn;
use uefi::boot::ScopedProtocol;
use uefi::{Event, Handle};
use uefi::proto::unsafe_protocol;
use uefi_raw::{Ipv4Address, Status};
use crate::uefi_raw::protocol::network::ip4::Ip4ModeData;

#[derive(Debug)]
#[repr(C)]
pub struct Udp4FragmentData {
    pub fragment_length: u32,
    pub fragment_buffer: *mut core::ffi::c_void,
}

impl Default for Udp4FragmentData {
    fn default() -> Self {
        Self {
            fragment_length: 0,
            fragment_buffer: core::ptr::null_mut(),
        }
    }
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4SessionData {
    pub source_address: Ipv4Address,
    pub source_port: u16,
    pub destination_address: Ipv4Address,
    pub destination_port: u16,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4ConfigData {
    pub accept_broadcast: bool,
    pub accept_promiscuous: bool,
    pub accept_any_port: bool,
    pub allow_duplicate_port: bool,
    pub type_of_service: u8,
    pub time_to_live: u8,
    pub do_not_fragment: bool,
    pub receive_timeout: u32,
    pub transmit_timeout: u32,
    pub use_default_address: bool,
    pub station_address: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub station_port: u16,
    pub remote_address: Ipv4Address,
    pub remote_port: u16,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4TransmitData<'a> {
    pub udp_session_data: Option<&'a Udp4SessionData>,
    pub gateway_address: Option<&'a Ipv4Address>,
    pub data_length: u32,
    pub fragment_count: u32,
    pub fragment_table: [Udp4FragmentData; 1],
}

#[derive(Debug)]
#[repr(C)]
pub struct Udp4ReceiveData {
    pub time_stamp: uefi_raw::time::Time,
    pub recycle_signal: uefi::Event,
    pub udp_session: Udp4SessionData,
    pub data_length: u32,
    pub fragment_count: u32,
    pub fragment_table: [Udp4FragmentData; 1],
}

pub struct Udp4ReceiveDataWrapperScoped(pub NonNull<Udp4ReceiveData>);

impl Udp4ReceiveDataWrapperScoped {
    pub unsafe fn new(data: &ManuallyDrop<Udp4ReceiveData>) -> Self {
        let data = NonNull::new(data.deref() as *const Udp4ReceiveData as *mut Udp4ReceiveData).expect("Failed to create NonNull");
        Self(data)
    }

    pub fn as_ref(&self) -> &Udp4ReceiveData {
        unsafe { self.0.as_ref() }
    }
}

impl Drop for Udp4ReceiveDataWrapperScoped {
    fn drop(&mut self) {
        let bt = uefi::table::system_table_raw().expect("Failed to get system table");
        let bt = unsafe { bt.as_ref() };

        let event = self.as_ref().recycle_signal.as_ptr();
        let status = unsafe { ((*bt.boot_services).signal_event)(event) };

        if status != Status::SUCCESS {
            warn!("Failed to signal event for UDP Packet disposal: {:?}", status);
        }
    }
}

#[repr(C)]
pub struct Udp4CompletionToken<'a> {
    pub event: uefi::Event,
    pub status: Status,
    pub packet: Udp4Packet<'a>,
}

#[repr(C)]
pub union Udp4Packet<'a> {
    pub rx_data: Option<&'a ManuallyDrop<Udp4ReceiveData>>,
    pub tx_data: Option<&'a Udp4TransmitData<'a>>,
}

#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("83f01464-99bd-45e5-b383-af6305d8e9e6")]
pub struct UDP4ServiceBindingProtocol {
    create_child: extern "efiapi" fn(
        this: &Self,
        out_child_handle: &mut *mut c_void,
    ) -> Status,

    destroy_child: extern "efiapi" fn(
        this: &Self,
        child_handle: Handle,
    ) -> Status,
}

impl UDP4ServiceBindingProtocol {
    pub fn create_child(&self) -> Result<BindingProtocol<UDP4ServiceBindingProtocol>, Status> {
        let mut handle = core::ptr::null_mut();
        let result = (self.create_child)(self, &mut handle);
        if result == Status::SUCCESS {
            Ok(BindingProtocol {
                binder: self,
                handle: unsafe { Handle::from_ptr(handle).unwrap() },
            })
        } else {
            Err(result)
        }
    }
}

pub trait BindingProtocolTrait {
    fn destroy_child(&self, child_handle: Handle) -> Status;
}

impl BindingProtocolTrait for UDP4ServiceBindingProtocol {
    fn destroy_child(&self, child_handle: Handle) -> Status {
        (self.destroy_child)(self, child_handle)
    }
}

pub struct BindingProtocol<'a, B: BindingProtocolTrait> {
    binder: &'a B,
    handle: Handle,
}

impl<'a, B: BindingProtocolTrait> BindingProtocol<'a, B> {
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }
}

impl<'a, B: BindingProtocolTrait> Drop for BindingProtocol<'a, B> {
    fn drop(&mut self) {
        let _ = self.binder.destroy_child(self.handle);
    }
}