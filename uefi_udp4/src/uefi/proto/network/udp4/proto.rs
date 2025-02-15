use crate::uefi::proto::network::udp4::managed_event::ManagedEvent;
use crate::uefi_raw::protocol::network::ip4::{
    Ip4ConfigData, Ip4IcmpType, Ip4ModeData, Ip4RouteTable, ManagedNetworkConfigData,
};
use crate::uefi_raw::protocol::network::udp4::{
    Udp4CompletionToken, Udp4ConfigData, Udp4FragmentData, Udp4Packet,
    Udp4ReceiveDataWrapperScoped, Udp4SessionData, Udp4TransmitData,
};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::AtomicBool;
use log::{trace, warn};
use uefi::boot::TimerTrigger;
use uefi::proto::unsafe_protocol;
use uefi_raw::table::boot::{EventType, Tpl};
use uefi_raw::{Ipv4Address, Status};

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

#[derive(Debug)]
#[allow(dead_code)]
pub struct Ip4ModeDataSafe<'a> {
    is_started: bool,
    max_packet_size: u32,
    config_data: Ip4ConfigData,
    is_configured: bool,
    group_table: &'a [Ipv4Address],
    ip4_route_table: &'a [Ip4RouteTable],
    icmp_type_list: &'a [Ip4IcmpType],
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

#[derive(Debug)]
pub struct UdpConfiguration<'a> {
    pub udp4config_data: Udp4ConfigData,
    pub ip4_mode_data: Ip4ModeDataSafe<'a>,
    pub managed_network_config_data: ManagedNetworkConfigData,
}

impl UDP4Protocol {
    pub fn reset(&mut self) -> Result<(), Status> {
        let result = (self.configure_fn)(self, None);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    pub fn configure(&mut self, connection_mode: &Udp4ConfigData) -> Result<(), Status> {
        let result = (self.configure_fn)(self, Some(connection_mode));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

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

    pub fn leave_all_multicast_groups(&mut self) -> Result<(), Status> {
        let result = (self.groups_fn)(self, false, None);
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    pub fn join_multicast_group(&mut self, multicast_address: &Ipv4Address) -> Result<(), Status> {
        let result = (self.groups_fn)(self, true, Some(multicast_address));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

    pub fn leave_multicast_group(&mut self, multicast_address: &Ipv4Address) -> Result<(), Status> {
        let result = (self.groups_fn)(self, false, Some(multicast_address));
        if result != Status::SUCCESS {
            return Err(result);
        }
        Ok(())
    }

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

    pub fn poll(&mut self) -> Result<(), Status> {
        match (self.poll_fn)(self) {
            Status::SUCCESS => Ok(()),
            status => Err(status),
        }
    }

    pub fn transmit(
        &mut self,
        session_data: Option<&Udp4SessionData>,
        gateway: Option<&Ipv4Address>,
        data: &[u8],
    ) -> Result<(), Status> {
        let sent = Arc::new(AtomicBool::new(false));
        let sent_clone = sent.clone();

        let event = ManagedEvent::new(EventType::NOTIFY_SIGNAL, move |_| {
            sent_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
        });

        let udp_transmit_data = Udp4TransmitData {
            udp_session_data: session_data,
            gateway_address: gateway,
            data_length: data.len() as u32,
            fragment_count: 1,
            fragment_table: [Udp4FragmentData {
                fragment_buffer: data.as_ptr() as *mut c_void,
                fragment_length: data.len() as u32,
            }],
        };

        let packet = Udp4Packet {
            tx_data: Some(&udp_transmit_data),
        };

        let mut token = Udp4CompletionToken {
            event: unsafe { event.event.unsafe_clone() },
            status: Status::SUCCESS,
            packet,
        };

        let result = (self.transmit_fn)(self, &mut token);
        if result != Status::SUCCESS {
            trace!("Failed to transmit data: {:?}", result);
            return Err(result);
        }

        while sent.load(core::sync::atomic::Ordering::Relaxed) == false {
            let _ = self.poll();
        }

        drop(event);

        if token.status != Status::SUCCESS {
            trace!("Transmit failed post-late: {:?}", token.status);
            return Err(token.status);
        }

        Ok(())
    }

    pub fn receive(&mut self, timeout_millis: Option<u64>) -> Result<Vec<u8>, Status> {
        let receive = Arc::new(AtomicBool::new(false));
        let receive_clone = receive.clone();

        let timeout = Arc::new(AtomicBool::new(false));
        let timeout_clone = timeout.clone();

        let event = ManagedEvent::new(EventType::NOTIFY_SIGNAL, move |_| {
            receive_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
        });

        let timeout_event = timeout_millis.map(|timeout| {
            (
                timeout,
                ManagedEvent::new(EventType::TIMER, move |_| {
                    timeout_clone.swap(true, core::sync::atomic::Ordering::Relaxed);
                }),
            )
        });

        let packet = Udp4Packet { rx_data: None };

        let mut token = Udp4CompletionToken {
            event: unsafe { event.event.unsafe_clone() },
            status: Status::SUCCESS,
            packet,
        };

        let result = (self.receive_fn)(self, &mut token);

        if result != Status::SUCCESS {
            trace!("Failed to receive data: {:?}", result);
            return Err(result);
        }

        if let Some((timeout_millis, timer_event)) = &timeout_event {
            uefi::boot::set_timer(
                &timer_event.event,
                TimerTrigger::Relative(timeout_millis * 10),
            )
            .expect("timer setup failed");
        }

        while receive.load(core::sync::atomic::Ordering::Relaxed) == false
            && timeout.load(core::sync::atomic::Ordering::Relaxed) == false
        {
            let _ = self.poll();
        }

        if let Some((_timeout_millis, timer_event)) = &timeout_event {
            uefi::boot::set_timer(&timer_event.event, TimerTrigger::Cancel)
                .expect("timer setup failed");
        }

        let guard = unsafe { uefi::boot::raise_tpl(Tpl::HIGH_LEVEL) }; // we do not want the packet to be processed while we are canceling it
        if receive.load(core::sync::atomic::Ordering::Relaxed) == false
            && timeout.load(core::sync::atomic::Ordering::Relaxed) == true
        {
            trace!(
                "Receive timed out: {:?}",
                (self.cancel_fn)(self, Some(&mut token))
            );
            return Err(Status::TIMEOUT);
        }
        drop(guard);

        drop(timeout_event);
        drop(event);

        if token.status != Status::SUCCESS {
            trace!("Receive failed post-late: {:?}", token.status);
            return Err(token.status);
        }

        assert!(unsafe { token.packet.rx_data }.is_some());

        let rx_data = unsafe { Udp4ReceiveDataWrapperScoped::new(token.packet.rx_data.unwrap()) };

        let data_length = rx_data.data_length;
        if data_length > 8192 {
            warn!("Buffer too small for received data");
            return Err(Status::BUFFER_TOO_SMALL);
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
                return Err(Status::BUFFER_TOO_SMALL);
            }

            result.extend_from_slice(fragment_buffer);
        }

        assert_eq!(result.len(), data_length as usize);

        Ok(result)
    }
}
