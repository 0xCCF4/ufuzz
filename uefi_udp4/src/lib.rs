//! UEFI UDP4 Protocol Library
//!
//! This library provides a safe and ergonomic interface for working with the UEFI UDP4 protocol.
//! It includes both raw protocol bindings and higher-level abstractions for UDP networking in UEFI.
//!
//! # Main Components
//!
//! - [`uefi::proto::network::udp4`]: High-level UDP4 protocol interface
//! - [`uefi_raw::protocol::network::udp4`]: Raw UDP4 protocol bindings
//! - [`uefi_raw::protocol::network::ip4`]: Raw IPv4 protocol bindings
//!

#![no_std]

extern crate alloc;

use ::uefi_raw::Ipv4Address;

/// Extension trait for IPv4 addresses
///
/// This trait provides additional methods for creating and manipulating IPv4 addresses
/// beyond what's available in the raw UEFI bindings.
pub trait Ipv4AddressExt {
    /// Creates a new IPv4 address from four octets
    ///
    /// # Arguments
    ///
    /// * `b1` - First octet
    /// * `b2` - Second octet
    /// * `b3` - Third octet
    /// * `b4` - Fourth octet
    ///
    /// # Returns
    ///
    /// Returns a new IPv4 address with the specified octets
    fn new(b1: u8, b2: u8, b3: u8, b4: u8) -> Self;

    /// Creates a new IPv4 address with all octets set to zero
    ///
    /// # Returns
    ///
    /// Returns a new IPv4 address with all octets set to zero
    fn zero() -> Self;
}

impl Ipv4AddressExt for Ipv4Address {
    fn new(b1: u8, b2: u8, b3: u8, b4: u8) -> Self {
        Self([b1, b2, b3, b4])
    }

    fn zero() -> Self {
        Self([0, 0, 0, 0])
    }
}

/// High-level UEFI protocol interfaces
pub mod uefi {
    /// Protocol interfaces
    pub mod proto {
        /// Network protocol interfaces
        pub mod network {
            /// UDP4 protocol interface
            pub mod udp4;
        }
    }
}

/// Raw UEFI protocol bindings
pub mod uefi_raw {
    /// Protocol bindings
    pub mod protocol {
        /// Network protocol bindings
        pub mod network {
            /// IPv4 protocol bindings
            pub mod ip4;
            /// UDP4 protocol bindings
            pub mod udp4;
        }
    }
}
