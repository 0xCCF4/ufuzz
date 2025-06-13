//! Raw IPv4 Protocol Bindings
//!
//! This module provides raw bindings to the UEFI IPv4 protocol, which implements the Internet Protocol version 4.
//! These bindings closely match the UEFI specification and provide low-level access to IPv4 functionality.

use uefi_raw::Ipv4Address;

/// IPv4 route table entry
///
/// This struct represents a single entry in the IPv4 routing table, containing
/// subnet address, subnet mask, and gateway address information.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Ip4.h#L130>
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
#[repr(C)]
pub struct Ip4RouteTable {
    /// Subnet address
    pub subnet_addr: Ipv4Address,
    /// Subnet mask
    pub subnet_mask: Ipv4Address,
    /// Gateway address
    pub gateway_addr: Ipv4Address,
}

/// IPv4 mode data
///
/// This struct contains the current state and configuration of an IPv4 instance,
/// including routing tables, group memberships, and ICMP settings.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Ip4.h#L141>
#[derive(Debug)]
#[repr(C)]
pub struct Ip4ModeData {
    /// Whether the IPv4 instance is started
    pub is_started: bool,
    /// Maximum packet size supported
    pub max_packet_size: u32,
    /// Current configuration data
    pub config_data: Ip4ConfigData,
    /// Whether the instance is configured
    pub is_configured: bool,
    /// Number of multicast groups
    pub group_count: u32,
    /// Pointer to multicast group table
    pub group_table: *const Ipv4Address,
    /// Number of routes in the routing table
    pub route_count: u32,
    /// Pointer to the routing table
    pub ip4_route_table: *const Ip4RouteTable,
    /// Number of ICMP types
    pub icmp_type_count: u32,
    /// Pointer to the ICMP type list
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

/// IPv4 configuration data
///
/// This struct contains all the configuration options for an IPv4 instance.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Ip4.h#L58>
#[derive(Debug, Default)]
#[repr(C)]
pub struct Ip4ConfigData {
    /// Default protocol number
    pub default_protocol: u8,
    /// Whether to accept any protocol
    pub accept_any_protocol: bool,
    /// Whether to accept ICMP errors
    pub accept_icmp_errors: bool,
    /// Whether to accept broadcast packets
    pub accept_broadcast: bool,
    /// Whether to accept promiscuous packets
    pub accept_promiscuous: bool,
    /// Whether to use the default address
    pub use_default_address: bool,
    /// Local station address
    pub station_address: Ipv4Address,
    /// Subnet mask
    pub subnet_mask: Ipv4Address,
    /// Type of service value for outgoing packets
    pub type_of_service: u8,
    /// Time to live value for outgoing packets
    pub time_to_live: u8,
    /// Whether to set the don't fragment flag
    pub do_not_fragment: bool,
    /// Whether to handle raw data
    pub raw_data: bool,
    /// Timeout for receive operations in microseconds
    pub receive_timeout: u32,
    /// Timeout for transmit operations in microseconds
    pub transmit_timeout: u32,
}

/// ICMP type and code
///
/// This struct represents an ICMP message type and its corresponding code.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/Ip4.h#L136>
#[derive(Debug, Default)]
#[repr(C)]
pub struct Ip4IcmpType {
    /// ICMP message type
    pub _type: u8,
    /// ICMP message code
    pub code: u8,
}

/// Managed network configuration data
///
/// This struct contains configuration options for the managed network protocol,
/// which provides a common interface for network protocols.
///
/// <https://github.com/tianocore/edk2/blob/1b26c4b73b27386f187fabe37810d3e2c055dc43/MdePkg/Include/Protocol/ManagedNetwork.h#L30>
#[derive(Debug, Default)]
#[repr(C)]
pub struct ManagedNetworkConfigData {
    /// Timeout for received packets in microseconds
    pub received_queue_timeout_value: u32,
    /// Timeout for transmitted packets in microseconds
    pub transmit_queue_timeout_value: u32,
    /// Protocol type filter
    pub protocol_type_filter: u16,
    /// Whether to enable unicast receive
    pub enable_unicast_receive: bool,
    /// Whether to enable multicast receive
    pub enable_multicast_receive: bool,
    /// Whether to enable broadcast receive
    pub enable_broadcast_receive: bool,
    /// Whether to enable promiscuous receive
    pub enable_promiscuous_receive: bool,
    /// Whether to flush queues on reset
    pub flush_queues_on_reset: bool,
    /// Whether to enable receive timestamps
    pub enable_receive_timestamps: bool,
    /// Whether to disable background polling
    pub disable_background_polling: bool,
}
