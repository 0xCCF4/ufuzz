#![no_std]

extern crate alloc;

use ::uefi_raw::Ipv4Address;

pub trait Ipv4AddressExt {
    fn new(b1: u8, b2: u8, b3: u8, b4: u8) -> Self;
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

pub mod uefi {
    pub mod proto {
        pub mod network {
            pub mod udp4;
        }
    }
}

pub mod uefi_raw {
    pub mod protocol {
        pub mod network {
            pub mod udp4;
            pub mod ip4;
        }
    }
}