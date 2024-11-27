use core::ptr::NonNull;
use custom_processing_unit::call_custom_ucode_function;
use data_types::addresses::{Address, MSRAMInstructionPartReadAddress, UCInstructionAddress};
use data_types::UcodePatchEntry;
use crate::page_allocation::PageAllocation;

const NUMBER_OF_ADDRESSES: usize = UCInstructionAddress::MAX.to_const()+1;

pub fn dump_ucode<T: Into<UCInstructionAddress>>(dump_func: T) -> Result<[UcodePatchEntry; NUMBER_OF_ADDRESSES/4], &'static str> {
    //                                                   num * 64bit=8bytes
    let page = PageAllocation::alloc((NUMBER_OF_ADDRESSES*8)/4096 + 1).map_err(|_| "Unable to allocate memory")?;

    let func = call_custom_ucode_function(dump_func.into(), [page.address(), 0, 0]);

    if func.rax != 0x442100004421 {
        return Err("Failed to dump ucode");
    }

    let page_base: NonNull<u64> = page.ptr().cast();

    let mut raw_data = [0u64; NUMBER_OF_ADDRESSES];

    for (i, value) in raw_data.iter_mut().enumerate() {
        unsafe {
            *value = page_base.as_ptr().add(i).read_volatile();
        }
    }

    page.dealloc();

    let instruction_dump = &raw_data[0..0x17F];
    let sequence_dump = &raw_data[0x180..0x1FF];

    let mut result: [UcodePatchEntry; NUMBER_OF_ADDRESSES/4] = [[0usize;4]; NUMBER_OF_ADDRESSES/4];
    for (i, row) in result.iter_mut().enumerate() {
        row[3] = sequence_dump[i] as usize;

        for (j, col) in row.iter_mut().enumerate() {
            let address = UCInstructionAddress::ZERO + i*4 + j;
            let read_address = MSRAMInstructionPartReadAddress::from(address);

            *col = instruction_dump[read_address.address()] as usize;
        }
    }

    Ok(result)
}
