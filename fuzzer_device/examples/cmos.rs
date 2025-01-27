#![no_main]
#![no_std]

mod cmos;

extern crate alloc;

use alloc::string::{String, ToString};
use core::fmt::Debug;
use uefi::{entry, println, Status};
use uefi::boot::ScopedProtocol;
use uefi::proto::loaded_image::LoadedImage;
use crate::cmos::CMOS;
use alloc::vec::Vec;

const STRING_LEN: usize = 59;
#[repr(C)]
struct CmosActualData {
    length: u8,
    str: [u8; STRING_LEN],
}
const _:() = CMOS::<CmosActualData>::size_check();

impl CmosActualData {
    fn update(&mut self, new_str: &str) {
        let new_str = new_str.as_bytes();
        let new_len = new_str.len().min(STRING_LEN);
        self.length = new_len as u8;
        self.str[..new_len].copy_from_slice(&new_str[..new_len]);
    }

    fn current_string(&self) -> String {
        String::from_utf8_lossy(&self.str[..self.length as usize]).to_string()
    }
}

impl Default for CmosActualData {
    fn default() -> Self {
        let mut result =
            Self {
                str: [0; STRING_LEN],
                length: 0,
            };

        result.update("Hello World!");

        result
    }
}

impl Debug for CmosActualData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.length == 0 {
            write!(f, "<Empty>")
        } else {
            write!(f, "{}", &self.current_string())
        }
    }
}

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    println!("Hello world!");

    let nmi = cmos::is_nmi_disabled();

    println!("NMI-disabled: {}", nmi);

    println!("Reading CMOS RAM...");
    let mut cmos = CMOS::<CmosActualData>::new(nmi);
    cmos.read_cmos_ram(nmi);

    // println!("Raw data: {:?} {:?}", String::from_utf8(cmos.raw_data().iter().filter(|x|x.is_ascii_alphanumeric()).cloned().collect()).unwrap(), &cmos.raw_data());

    if !cmos.checksum_valid() {
        println!("Checksum not valid, resetting CMOS RAM...");
        cmos.reset();
    }

    println!("Current CMOS RAM: {:?}", &cmos.data().unwrap().current_string());

    let loaded_image_proto: ScopedProtocol<LoadedImage> =
        match uefi::boot::open_protocol_exclusive(uefi::boot::image_handle()) {
            Err(err) => {
                println!("Failed to open image protocol: {:?}", err);
                return Status::ABORTED;
            }
            Ok(loaded_image_proto) => loaded_image_proto,
        };
    let args = match loaded_image_proto.load_options_as_bytes().map(|options| {
        options
            .into_iter()
            .enumerate()
            .filter_map(|(i, x)| if i % 2 == 0 {Some(x)} else {None})
            .map(|p| if *p == 0 { ' ' as u8 } else {*p})
            .collect::<Vec<_>>()
    }) {
        None => {
            println!("No args set.");
            return Status::ABORTED;
        }
        Some(options) => {
            let slice = options.as_slice();
            String::from_utf8_lossy(slice).to_string()
        },
    };

    let args = args.split_once(' ').map(|(_, a)| a).unwrap_or(args.as_str()).trim();
    let args = args.rsplit_once(' ').map(|(a, _)| a).unwrap_or("").trim();

    println!("New text: {}", args);

    let mut data = cmos.data_mut().unwrap();

    data.update(&args);
    drop(data);

    cmos.write_cmos_ram(nmi);

    Status::SUCCESS
}
