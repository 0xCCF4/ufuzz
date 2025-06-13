//! UDP4 Protocol Interface
//!
//! This module provides a safe interface for working with the UEFI UDP4 protocol.
//! It includes functionality for configuring UDP sessions, transmitting and receiving packets.

use crate::uefi::proto::network::udp4::managed_event::ManagedEvent;
use crate::uefi_raw::protocol::network::ip4::{
    Ip4ConfigData, Ip4IcmpType, Ip4ModeData, Ip4RouteTable, ManagedNetworkConfigData,
};
use crate::uefi_raw::protocol::network::udp4::{
    Udp4CompletionToken, Udp4ConfigData, Udp4FragmentData, Udp4Packet,
    Udp4ReceiveDataWrapperScoped, Udp4SessionData, Udp4TransmitData,
};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::error::Error;
use core::ffi::c_void;
use core::fmt::Display;
use core::mem::transmute;
use core::pin::Pin;
use core::sync::atomic::AtomicBool;
use log::{trace, warn};
use uefi::boot::TimerTrigger;
use uefi::proto::unsafe_protocol;
use uefi_raw::table::boot::{EventType, Tpl};
use uefi_raw::{Ipv4Address, Status};

/// UDP4 Protocol Interface
///
/// This struct provides low-level interface for working with the UEFI UDP4 protocol.
///
/// # Safety
///
/// This protocol is marked as unsafe because it directly interfaces with UEFI system calls.
/// Users should ensure proper initialization and cleanup of protocol instances, validate
/// the correctness of the provided arguments, and handle errors appropriately.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Udp4.h#L423>
#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("3ad9df29-4501-478d-b1f8-7f7fe70e50f3")]
pub struct UDP4Protocol {
    get_mode_data_fn: extern "efiapi" fn(
        this: &mut Self,
        out_config_data: Option<&mut Udp4ConfigData>,
        out_ip4_mode_data: Option<&mut Ip4ModeData>,
        out_managed_network_config_data: Option<&mut ManagedNetworkConfigData>,
        out_simple_network_mode: Option<&mut core::ffi::c_void>,
    ) -> Status,

    configure_fn:
        extern "efiapi" fn(this: &mut Self, config_data: Option<&Udp4ConfigData>) -> Status,

    groups_fn: extern "efiapi" fn(
        this: &mut Self,
        join_flag: bool,
        multicast_address: Option<&Ipv4Address>,
    ) -> Status,

    routes_fn: extern "efiapi" fn(
        this: &mut Self,
        delete_route: bool,
        subnet_address: &Ipv4Address,
        subnet_mask: &Ipv4Address,
        gateway_address: &Ipv4Address,
    ) -> Status,

    transmit_fn: extern "efiapi" fn(this: &mut Self, token: &mut Udp4CompletionToken) -> Status,

    receive_fn: extern "efiapi" fn(this: &mut Self, token: &mut Udp4CompletionToken) -> Status,

    cancel_fn:
        extern "efiapi" fn(this: &mut Self, token: Option<&mut Udp4CompletionToken>) -> Status,

    poll_fn: extern "efiapi" fn(this: &mut Self) -> Status,
}

/// Safe wrapper for IPv4 mode data
///
/// This struct provides a safe interface to the IPv4 mode data, ensuring proper
/// handling of raw pointers and memory safety.
///
/// Todo: does we need to free the table pointers?
#[derive(Debug)]
#[allow(dead_code)]
pub struct Ip4ModeDataSafe<'a> {
    /// Whether the IPv4 instance is started
    pub is_started: bool,
    /// Maximum packet size supported
    pub max_packet_size: u32,
    /// Current configuration data
    pub config_data: Ip4ConfigData,
    /// Whether the instance is configured
    pub is_configured: bool,
    /// Multicast group table
    pub group_table: &'a [Ipv4Address],
    /// IPv4 routing table
    pub ip4_route_table: &'a [Ip4RouteTable],
    /// ICMP type list
    pub icmp_type_list: &'a [Ip4IcmpType],
}

impl From<Ip4ModeData> for Ip4ModeDataSafe<'_> {
    fn from(value: Ip4ModeData) -> Self {
        unsafe {
            Ip4ModeDataSafe {
                group_table: &*core::ptr::slice_from_raw_parts(
                    value.group_table,
                    value.group_count as usize,
                ),
                ip4_route_table: &*core::ptr::slice_from_raw_parts(
                    value.ip4_route_table,
                    value.route_count as usize,
                ),
                icmp_type_list: &*core::ptr::slice_from_raw_parts(
                    value.icmp_type_list,
                    value.icmp_type_count as usize,
                ),
                is_started: value.is_started,
                max_packet_size: value.max_packet_size,
                config_data: value.config_data,
                is_configured: value.is_configured,
            }
        }
    }
}

/// UDP Configuration Data
///
/// This struct contains all the configuration data for a UDP session, including
/// UDP4 configuration, IPv4 mode data, and managed network configuration.
#[derive(Debug)]
pub struct UdpConfiguration<'a> {
    /// UDP4 configuration data
    pub udp4config_data: Udp4ConfigData,
    /// IPv4 mode data
    pub ip4_mode_data: Ip4ModeDataSafe<'a>,
    /// Managed network configuration data
    pub managed_network_config_data: ManagedNetworkConfigData,
}

/// Transmission Error Types
///
/// This enum represents the possible errors that can occur during UDP packet transmission.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub enum TransmitError {
    /// Transmission failed with the given status
    TransmitFailed(Status),
    /// Post-transmission operation failed with the given status
    PostLate(Status),
}

impl Display for TransmitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TransmitError::TransmitFailed(status) => write!(f, "Transmit failed: {:x?}", status),
            TransmitError::PostLate(status) => {
                write!(f, "Transmit post-late failed: {:x?}", status)
            }
        }
    }
}

impl Error for TransmitError {}

/// Reception Error Types
///
/// This enum represents the possible errors that can occur during UDP packet reception.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub enum ReceiveError {
    /// Reception failed with the given status
    ReceiveFailed(Status),
    /// Post-reception operation failed with the given status
    PostLate(Status),
    /// Reception timed out
    ReceiveTimeout,
    /// Receive buffer was too small for the received data
    ReceiveBufferTooSmall(usize),
}

impl Display for ReceiveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReceiveError::ReceiveFailed(status) => write!(f, "Receive failed: {:x?}", status),
            ReceiveError::PostLate(status) => write!(f, "Receive post-late failed: {:x?}", status),
            ReceiveError::ReceiveTimeout => write!(f, "Receive timed out"),
            ReceiveError::ReceiveBufferTooSmall(size) => {
                write!(f, "Receive buffer too small: {}", size)
            }
        }
    }
}

impl Error for ReceiveError {}

impl UDP4Protocol {
    /// Resets the UDP4 protocol instance to its default state
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn reset(&mut self) -> Result<(), Status> {
        let result = (self.configure_fn)(self, None);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Cancels all pending operations
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn cancel_all(&mut self) -> Result<(), Status> {
        let result = (self.cancel_fn)(self, None);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Configures the UDP4 protocol instance
    ///
    /// # Arguments
    ///
    /// * `connection_mode` - The configuration data to apply
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn configure(&mut self, connection_mode: &Udp4ConfigData) -> Result<(), Status> {
        let result = (self.configure_fn)(self, Some(connection_mode));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Retrieves the current mode data for the UDP4 protocol instance
    ///
    /// # Returns
    ///
    /// Returns the current configuration data if successful, or an error status if the operation failed.
    pub fn get_mode_data(&mut self) -> Result<UdpConfiguration, Status> {
        let mut config_data = Udp4ConfigData::default();
        let mut ip4_mode_data = Ip4ModeData::default();
        let mut managed_network_config_data = ManagedNetworkConfigData::default();

        let result = (self.get_mode_data_fn)(
            self,
            Some(&mut config_data),
            Some(&mut ip4_mode_data),
            Some(&mut managed_network_config_data),
            None,
        );
        if result != Status::SUCCESS {
            return Err(result);
        }

        Ok(UdpConfiguration {
            udp4config_data: config_data,
            ip4_mode_data: Ip4ModeDataSafe::from(ip4_mode_data),
            managed_network_config_data,
        })
    }

    /// Leaves all multicast groups
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn leave_all_multicast_groups(&mut self) -> Result<(), Status> {
        let result = (self.groups_fn)(self, false, None);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Joins a multicast group
    ///
    /// # Arguments
    ///
    /// * `multicast_address` - The multicast address to join
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn join_multicast_group(&mut self, multicast_address: &Ipv4Address) -> Result<(), Status> {
        let result = (self.groups_fn)(self, true, Some(multicast_address));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Leaves a multicast group
    ///
    /// # Arguments
    ///
    /// * `multicast_address` - The multicast address to leave
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn leave_multicast_group(&mut self, multicast_address: &Ipv4Address) -> Result<(), Status> {
        let result = (self.groups_fn)(self, false, Some(multicast_address));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Adds a route to the routing table
    ///
    /// # Arguments
    ///
    /// * `subnet_address` - The subnet address
    /// * `subnet_mask` - The subnet mask
    /// * `gateway_address` - The gateway address
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn add_route(
        &mut self,
        subnet_address: &Ipv4Address,
        subnet_mask: &Ipv4Address,
        gateway_address: &Ipv4Address,
    ) -> Result<(), Status> {
        let result = (self.routes_fn)(self, false, subnet_address, subnet_mask, gateway_address);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Deletes a route from the routing table
    ///
    /// # Arguments
    ///
    /// * `subnet_address` - The subnet address
    /// * `subnet_mask` - The subnet mask
    /// * `gateway_address` - The gateway address
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn delete_route(
        &mut self,
        subnet_address: &Ipv4Address,
        subnet_mask: &Ipv4Address,
        gateway_address: &Ipv4Address,
    ) -> Result<(), Status> {
        let result = (self.routes_fn)(self, true, subnet_address, subnet_mask, gateway_address);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    /// Polls for pending operations
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or an error status if the operation failed.
    pub fn poll(&mut self) -> Result<(), Status> {
        match (self.poll_fn)(self) {
            Status::SUCCESS => Ok(()),
            status => Err(status),
        }
    }

    /// Creates a new UDP channel for sending and receiving packets
    ///
    /// # Returns
    ///
    /// Returns a new UDP channel instance.
    pub fn channel<'a>(&'a mut self) -> UdpChannel<'a> {
        UdpChannel::new(self)
    }
}

/// UDP Channel for sending and receiving packets
///
/// This struct provides a higher-level interface for sending and receiving UDP packets,
/// handling event management and completion tokens internally.
pub struct UdpChannel<'a> {
    /// The underlying UDP4 protocol instance
    udp: &'a mut UDP4Protocol,

    /// Result of receive operation
    status_rx_result: Arc<AtomicBool>,
    /// Result of transmit operation
    status_tx_result: Arc<AtomicBool>,
    /// Result of timeout operation
    status_timeout_result: Arc<AtomicBool>,

    /// Event for transmit operations
    #[allow(dead_code)]
    tx_event: ManagedEvent,
    /// Event for receive operations
    #[allow(dead_code)]
    rx_event: ManagedEvent,
    /// Event for timeout detection
    timeout_event: ManagedEvent,

    /// Completion token for receive operations
    rx_completion_token: Pin<Box<Udp4CompletionToken<'static>>>,
    /// Completion token for transmit operations
    tx_completion_token: Pin<Box<Udp4CompletionToken<'static>>>,
}

impl Drop for UdpChannel<'_> {
    fn drop(&mut self) {
        let _ = self.udp.cancel_all();
    }
}

impl<'a> UdpChannel<'a> {
    /// Creates a new UDP channel
    ///
    /// # Arguments
    ///
    /// * `udp` - The UDP4 protocol instance to use
    ///
    /// # Returns
    ///
    /// Returns a new UDP channel instance.
    pub fn new(udp: &'a mut UDP4Protocol) -> Self {
        let status_rx_result = Arc::new(AtomicBool::new(false));
        let status_tx_result = Arc::new(AtomicBool::new(false));
        let status_timeout_result = Arc::new(AtomicBool::new(false));

        let status_rx_result_clone = Arc::clone(&status_rx_result);
        let status_tx_result_clone = Arc::clone(&status_tx_result);
        let status_timeout_result_clone = Arc::clone(&status_timeout_result);

        let tx_event = ManagedEvent::new(EventType::NOTIFY_SIGNAL, move |_| {
            status_tx_result_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
        });
        let rx_event = ManagedEvent::new(EventType::NOTIFY_SIGNAL, move |_| {
            status_rx_result_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
        });
        let timeout_event =
            ManagedEvent::new(EventType::NOTIFY_SIGNAL | EventType::TIMER, move |_| {
                status_timeout_result_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
            });

        let tx_completion_token = Box::pin(Udp4CompletionToken {
            event: unsafe { tx_event.event.unsafe_clone() },
            status: Status::SUCCESS,
            packet: Udp4Packet { rx_data: None },
        });

        let rx_completion_token = Box::pin(Udp4CompletionToken {
            event: unsafe { rx_event.event.unsafe_clone() },
            status: Status::SUCCESS,
            packet: Udp4Packet { rx_data: None },
        });

        UdpChannel {
            udp,
            status_rx_result,
            status_tx_result,
            status_timeout_result,
            tx_event,
            rx_event,
            timeout_event,
            rx_completion_token,
            tx_completion_token,
        }
    }

    /// Transmits a UDP packet
    ///
    /// # Arguments
    ///
    /// * `session_data` - Optional session data for the packet
    /// * `gateway` - Optional gateway address
    /// * `data` - The data to transmit
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if successful, or a transmission error if the operation failed.
    pub fn transmit(
        &mut self,
        session_data: Option<&Udp4SessionData>,
        gateway: Option<&Ipv4Address>,
        data: &[u8],
    ) -> Result<(), TransmitError> {
        let udp_transmit_data = Box::pin(Udp4TransmitData {
            udp_session_data: session_data,
            gateway_address: gateway,
            data_length: data.len() as u32,
            fragment_count: 1,
            fragment_table: [Udp4FragmentData {
                fragment_buffer: data.as_ptr() as *mut c_void,
                fragment_length: data.len() as u32,
            }],
        });
        let transmit_data_ref: &Udp4TransmitData = &udp_transmit_data;
        let transmit_data_ref: &'static Udp4TransmitData = unsafe { transmute(transmit_data_ref) };

        let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.tx_completion_token));
        self.tx_completion_token.packet.tx_data = Some(transmit_data_ref);
        self.tx_completion_token.status = Status::SUCCESS;
        self.status_tx_result
            .store(false, core::sync::atomic::Ordering::Relaxed);

        let _ = self.udp.poll();
        let result = (self.udp.transmit_fn)(self.udp, &mut self.tx_completion_token);
        let _ = self.udp.poll();

        if result != Status::SUCCESS {
            trace!("Failed to transmit data: {:x?}", result);

            let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.tx_completion_token));
            self.tx_completion_token.packet.tx_data = None;
            drop(udp_transmit_data);

            return Err(TransmitError::TransmitFailed(result));
        }

        while self
            .status_tx_result
            .load(core::sync::atomic::Ordering::Relaxed)
            == false
        {
            let _ = self.udp.poll();
        }

        if self.tx_completion_token.status != Status::SUCCESS {
            trace!(
                "Transmit failed post-late: {:x?}",
                self.tx_completion_token.status
            );

            let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.tx_completion_token));
            self.tx_completion_token.packet.tx_data = None;
            drop(udp_transmit_data);

            return Err(TransmitError::PostLate(self.tx_completion_token.status));
        }

        let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.tx_completion_token));
        self.tx_completion_token.packet.tx_data = None;
        drop(udp_transmit_data);

        Ok(())
    }

    /// Receives a UDP packet
    ///
    /// # Arguments
    ///
    /// * `timeout_millis` - Optional timeout in milliseconds
    ///
    /// # Returns
    ///
    /// Returns the received data if successful, or a reception error if the operation failed.
    pub fn receive(&mut self, timeout_millis: Option<u64>) -> Result<Vec<u8>, ReceiveError> {
        let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.rx_completion_token));
        self.status_rx_result
            .store(false, core::sync::atomic::Ordering::Relaxed);
        self.status_timeout_result
            .store(false, core::sync::atomic::Ordering::Relaxed);

        self.rx_completion_token.packet.rx_data = None;
        self.rx_completion_token.status = Status::SUCCESS;

        let _ = self.udp.poll();
        let result = (self.udp.receive_fn)(self.udp, &mut self.rx_completion_token);
        let _ = self.udp.poll();

        if result != Status::SUCCESS {
            trace!("Failed to receive data: {:x?}", result);

            let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.rx_completion_token));

            return Err(ReceiveError::ReceiveFailed(result));
        }

        if let Some(timeout_millis) = &timeout_millis {
            uefi::boot::set_timer(
                &self.timeout_event.event,
                TimerTrigger::Relative(timeout_millis * 1000 * 10),
            )
            .expect("timer setup failed");
        }

        while self
            .status_rx_result
            .load(core::sync::atomic::Ordering::Relaxed)
            == false
            && self
                .status_timeout_result
                .load(core::sync::atomic::Ordering::Relaxed)
                == false
        {
            let _ = self.udp.poll();
        }

        let guard = unsafe { uefi::boot::raise_tpl(Tpl::HIGH_LEVEL) }; // we do not want the packet to be processed while we are canceling it

        if let Some(_timeout_millis) = timeout_millis {
            uefi::boot::set_timer(&self.timeout_event.event, TimerTrigger::Cancel)
                .expect("timer setup failed");
        }

        if self
            .status_rx_result
            .load(core::sync::atomic::Ordering::Relaxed)
            == false
        {
            // cancel the receive
            let cancel = (self.udp.cancel_fn)(self.udp, Some(&mut self.rx_completion_token));
            if cancel != Status::SUCCESS {
                trace!("Failed to cancel receive: {:x?}", cancel);
            }
        }

        if self
            .status_rx_result
            .load(core::sync::atomic::Ordering::Relaxed)
            == false
            && self
                .status_timeout_result
                .load(core::sync::atomic::Ordering::Relaxed)
                == true
        {
            // trace!("Receive timed out: {:x?}", (self.cancel_fn)(self, Some(&mut token)));
            let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.rx_completion_token));

            return Err(ReceiveError::ReceiveTimeout);
        }
        drop(guard);

        if self.rx_completion_token.status != Status::SUCCESS {
            // trace!("Receive failed post-late: {:x?}", token.status);
            let _ = (self.udp.cancel_fn)(self.udp, Some(&mut *self.rx_completion_token));

            return Err(ReceiveError::PostLate(self.rx_completion_token.status));
        }

        assert!(unsafe { self.rx_completion_token.packet.rx_data }.is_some());

        let rx_data = unsafe {
            Udp4ReceiveDataWrapperScoped::new(self.rx_completion_token.packet.rx_data.unwrap())
        };

        let data_length = rx_data.data_length;
        if data_length > 8192 {
            warn!("Buffer too small for received data");
            return Err(ReceiveError::ReceiveBufferTooSmall(data_length as usize));
        }
        let mut result = Vec::with_capacity(data_length as usize);

        let fragment_count = rx_data.fragment_count;
        let fragment_table = unsafe {
            &*core::ptr::slice_from_raw_parts(
                rx_data.fragment_table.as_ptr(),
                fragment_count as usize,
            )
        };

        for fragment in fragment_table {
            let fragment_buffer = unsafe {
                &*core::ptr::slice_from_raw_parts(
                    fragment.fragment_buffer as *const u8,
                    fragment.fragment_length as usize,
                )
            };

            if result.len() + fragment_buffer.len() > result.capacity() {
                warn!("Buffer too small for received fragment data");
                return Err(ReceiveError::ReceiveBufferTooSmall(
                    result.len() + fragment_buffer.len(),
                ));
            }

            result.extend_from_slice(fragment_buffer);
        }

        drop(rx_data);
        let _ = self.udp.poll();

        assert_eq!(result.len(), data_length as usize);

        Ok(result)
    }
}
