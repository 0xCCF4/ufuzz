use uefi_raw::Ipv4Address;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
#[repr(C)]
pub struct Ip4RouteTable {
    pub subnet_addr: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub gateway_addr: Ipv4Address,
}

#[derive(Debug)]
#[repr(C)]
pub struct Ip4ModeData {
    pub is_started: bool,
    pub max_packet_size: u32,
    pub config_data: Ip4ConfigData,
    pub is_configured: bool,
    pub group_count: u32,
    pub group_table: *const Ipv4Address,
    pub route_count: u32,
    pub ip4_route_table: *const Ip4RouteTable,
    pub icmp_type_count: u32,
    pub icmp_type_list: *const Ip4IcmpType,
}

impl Default for Ip4ModeData {
    fn default() -> Self {
        Self {
            is_started: false,
            max_packet_size: 0,
            config_data: Default::default(),
            is_configured: false,
            group_count: 0,
            group_table: core::ptr::null(),
            route_count: 0,
            ip4_route_table: core::ptr::null(),
            icmp_type_count: 0,
            icmp_type_list: core::ptr::null(),
        }
    }
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Ip4ConfigData {
    pub default_protocol: u8,
    pub accept_any_protocol: bool,
    pub accept_icmp_errors: bool,
    pub accept_broadcast: bool,
    pub accept_promiscuous: bool,
    pub use_default_address: bool,
    pub station_address: Ipv4Address,
    pub subnet_mask: Ipv4Address,
    pub type_of_service: u8,
    pub time_to_live: u8,
    pub do_not_fragment: bool,
    pub raw_data: bool,
    pub receive_timeout: u32,
    pub transmit_timeout: u32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Ip4IcmpType {
    pub _type: u8,
    pub code: u8,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct ManagedNetworkConfigData {
    pub received_queue_timeout_value: u32,
    pub transmit_queue_timeout_value: u32,
    pub protocol_type_filter: u16,
    pub enable_unicast_receive: bool,
    pub enable_multicast_receive: bool,
    pub enable_broadcast_receive: bool,
    pub enable_promiscuous_receive: bool,
    pub flush_queues_on_reset: bool,
    pub enable_receive_timestamps: bool,
    pub disable_background_polling: bool,
}