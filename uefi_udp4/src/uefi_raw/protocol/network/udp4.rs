//! Raw UDP4 Protocol Bindings
//!
//! This module provides raw bindings to the UEFI UDP4 protocol, which implements the User Datagram Protocol (UDP)
//! over IPv4. These bindings closely match the UEFI specification and provide low-level access to UDP functionality.
//!
//! Literature:
//! * <https://uefi.org/specs/UEFI/2.9_A/30_Network_Protocols_UDP_and_MTFTP.html>

use alloc::vec::Vec;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::ops::Deref;
use core::ptr::NonNull;
use log::warn;
use uefi::proto::{unsafe_protocol, Protocol};
use uefi::Handle;
use uefi_raw::{Ipv4Address, Status};

/// Represents a fragment of UDP data
///
/// This struct is used to describe a single fragment of UDP data in a packet.
/// It contains the length of the fragment and a pointer to the fragment's data.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L56>
#[derive(Debug)]
#[repr(C)]
pub struct Udp4FragmentData {
    /// Length of the fragment in bytes
    pub fragment_length: u32,
    /// Pointer to the fragment's data
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

/// Represents UDP session information
///
/// This struct contains the source and destination addresses and ports for a UDP session.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L61>
#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4SessionData {
    /// Source IPv4 address
    pub source_address: Ipv4Address,
    /// Source port number
    pub source_port: u16,
    /// Destination IPv4 address
    pub destination_address: Ipv4Address,
    /// Destination port number
    pub destination_port: u16,
}

/// UDP4 configuration data
///
/// This struct contains all the configuration options for a UDP4 instance.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L67>
#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4ConfigData {
    /// Whether to accept broadcast packets
    pub accept_broadcast: bool,
    /// Whether to accept promiscuous packets
    pub accept_promiscuous: bool,
    /// Whether to accept packets on any port
    pub accept_any_port: bool,
    /// Whether to allow duplicate port bindings
    pub allow_duplicate_port: bool,
    /// Type of service value for outgoing packets
    pub type_of_service: u8,
    /// Time to live value for outgoing packets
    pub time_to_live: u8,
    /// Whether to set the don't fragment flag
    pub do_not_fragment: bool,
    /// Timeout for receive operations in microseconds
    pub receive_timeout: u32,
    /// Timeout for transmit operations in microseconds
    pub transmit_timeout: u32,
    /// Whether to use the default address
    pub use_default_address: bool,
    /// Local station address
    pub station_address: Ipv4Address,
    /// Subnet mask
    pub subnet_mask: Ipv4Address,
    /// Local station port
    pub station_port: u16,
    /// Remote address
    pub remote_address: Ipv4Address,
    /// Remote port
    pub remote_port: u16,
}

/// UDP4 transmit data
///
/// This struct contains the data needed to transmit a UDP packet.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L94>
#[derive(Debug, Default)]
#[repr(C)]
pub struct Udp4TransmitData<'a> {
    /// Optional UDP session data
    pub udp_session_data: Option<&'a Udp4SessionData>,
    /// Optional gateway address
    pub gateway_address: Option<&'a Ipv4Address>,
    /// Total length of the data to transmit
    pub data_length: u32,
    /// Number of fragments
    pub fragment_count: u32,
    /// Array of fragment data
    pub fragment_table: [Udp4FragmentData; 1],
}

/// UDP4 receive data
///
/// This struct contains the data received from a UDP packet.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L102>
#[derive(Debug)]
#[repr(C)]
pub struct Udp4ReceiveData {
    /// Timestamp when the packet was received
    pub time_stamp: uefi_raw::time::Time,
    /// Event to signal when the data can be recycled
    pub recycle_signal: uefi::Event,
    /// UDP session information
    pub udp_session: Udp4SessionData,
    /// Total length of the received data
    pub data_length: u32,
    /// Number of fragments
    pub fragment_count: u32,
    /// Array of fragment data
    pub fragment_table: [Udp4FragmentData; 1],
}

/// A wrapper for UDP4 receive data that handles cleanup
///
/// This struct ensures that the recycle signal is properly signaled when the receive data is dropped.
pub struct Udp4ReceiveDataWrapperScoped(pub NonNull<Udp4ReceiveData>);

impl Udp4ReceiveDataWrapperScoped {
    /// Creates a new wrapper for UDP4 receive data
    ///
    /// # Arguments
    ///
    /// * `data` - The receive data to wrap
    ///
    /// # Safety
    ///
    /// The caller must ensure that the data remains valid for the lifetime of the wrapper.
    pub unsafe fn new(data: &ManuallyDrop<Udp4ReceiveData>) -> Self {
        let data = NonNull::new(data.deref() as *const Udp4ReceiveData as *mut Udp4ReceiveData)
            .expect("Failed to create NonNull");
        Self(data)
    }
}

impl AsRef<Udp4ReceiveData> for Udp4ReceiveDataWrapperScoped {
    fn as_ref(&self) -> &Udp4ReceiveData {
        unsafe { self.0.as_ref() }
    }
}

impl Deref for Udp4ReceiveDataWrapperScoped {
    type Target = Udp4ReceiveData;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl Drop for Udp4ReceiveDataWrapperScoped {
    fn drop(&mut self) {
        let bt = uefi::table::system_table_raw().expect("Failed to get system table");
        let bt = unsafe { bt.as_ref() };

        let event = self.as_ref().recycle_signal.as_ptr();

        let status = unsafe { ((*bt.boot_services).signal_event)(event) };

        if status != Status::SUCCESS {
            warn!(
                "Failed to signal event for UDP Packet disposal: {:?}",
                status
            );
        }
    }
}

/// UDP4 completion token
///
/// This struct is used to track the completion of UDP operations.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L111>
#[repr(C)]
pub struct Udp4CompletionToken<'a> {
    /// Event to signal when the operation completes
    pub event: uefi::Event,
    /// Status of the operation
    pub status: Status,
    /// The UDP packet data
    pub packet: Udp4Packet<'a>,
}

/// UDP4 packet data
///
/// This union contains either receive or transmit data for a UDP packet.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L111>
#[repr(C)]
pub union Udp4Packet<'a> {
    /// Receive data
    pub rx_data: Option<&'a ManuallyDrop<Udp4ReceiveData>>,
    /// Transmit data
    pub tx_data: Option<&'a Udp4TransmitData<'a>>,
}

/// UDP4 Service Binding Protocol
///
/// This protocol is used to create and destroy UDP4 protocol instances.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L22>
#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("83f01464-99bd-45e5-b383-af6305d8e9e6")]
pub struct UDP4ServiceBindingProtocol {
    create_child: extern "efiapi" fn(this: &Self, out_child_handle: &mut *mut c_void) -> Status,
    destroy_child: extern "efiapi" fn(this: &Self, child_handle: Handle) -> Status,
}

impl UDP4ServiceBindingProtocol {
    /// Creates a new UDP4 protocol instance
    ///
    /// # Returns
    ///
    /// Returns a scoped binding protocol that will automatically clean up when dropped.
    pub fn create_child(
        &self,
    ) -> Result<ScopedBindingProtocol<UDP4ServiceBindingProtocol>, Status> {
        let mut handle = core::ptr::null_mut();
        let result = (self.create_child)(self, &mut handle);
        if result == Status::SUCCESS {
            Ok(ScopedBindingProtocol {
                binders: uefi::boot::find_handles::<UDP4ServiceBindingProtocol>()
                    .expect("We are calling from this"),
                handle: unsafe { Handle::from_ptr(handle).unwrap() },
                phantom_data: PhantomData,
            })
        } else {
            Err(result)
        }
    }
}

/// Trait for binding protocols that can create and destroy child handles
pub trait BindingProtocolTrait: Protocol {
    /// Destroys a child handle
    ///
    /// # Arguments
    ///
    /// * `child_handle` - The handle to destroy
    ///
    /// # Returns
    ///
    /// Returns the status of the operation
    fn destroy_child(&self, child_handle: Handle) -> Status;
}

impl BindingProtocolTrait for UDP4ServiceBindingProtocol {
    fn destroy_child(&self, child_handle: Handle) -> Status {
        (self.destroy_child)(self, child_handle)
    }
}

/// A scoped wrapper for binding protocols
///
/// This struct ensures that child handles are properly destroyed when dropped.
pub struct ScopedBindingProtocol<B: BindingProtocolTrait> {
    binders: Vec<Handle>,
    handle: Handle,
    phantom_data: PhantomData<B>,
}

impl<B: BindingProtocolTrait> ScopedBindingProtocol<B> {
    /// Gets the handle for this binding protocol
    ///
    /// # Returns
    ///
    /// Returns a clone of the handle
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }
}

impl<B: BindingProtocolTrait> Drop for ScopedBindingProtocol<B> {
    fn drop(&mut self) {
        for binder in &self.binders {
            let binder = uefi::boot::open_protocol_exclusive::<B>(*binder)
                .expect("Failed to open protocol: is it already opened?");

            let status = binder.destroy_child(self.handle);
            if status == Status::SUCCESS {
                return;
            }
        }
        panic!("Failed to destroy child handle");
    }
}
