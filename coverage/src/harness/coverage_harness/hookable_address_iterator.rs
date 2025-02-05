use crate::harness::coverage_harness::modification_engine;
use crate::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use alloc::rc::Rc;
use alloc::vec::Vec;
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use ucode_dump::RomDump;

#[derive(Debug, Clone)]
pub struct HookableAddressIterator {
    addresses: Rc<Vec<UCInstructionAddress>>,
    chunk_size: usize,
}

impl HookableAddressIterator {
    pub fn construct<F: Fn(UCInstructionAddress) -> bool>(
        rom: &RomDump,
        modification_engine_settings: &ModificationEngineSettings,
        chunk_size: usize,
        filter: F,
    ) -> Self {
        let hookable_addresses = (0..0x7c00)
            .filter(|a| a % 2 == 0)
            .map(UCInstructionAddress::from_const)
            .filter(|f| filter(*f))
            .filter(|a| {
                modification_engine::modify_triad_for_hooking(*a, rom, modification_engine_settings)
                    .is_ok()
                    && modification_engine::modify_triad_for_hooking(
                        *a + 1,
                        rom,
                        modification_engine_settings,
                    )
                    .is_ok()
            })
            .collect_vec();

        // now reorder, such that x and x+2 are not within chunk_size of each other

        let chunked = hookable_addresses.chunks(chunk_size).collect_vec();
        let mut result = Vec::with_capacity(hookable_addresses.len());

        for i in 0..chunk_size {
            for chunk in chunked.iter() {
                if let Some(address) = chunk.get(i) {
                    result.push(*address);
                }
            }
        }

        HookableAddressIterator {
            addresses: Rc::new(result),
            chunk_size,
        }
    }

    pub fn len(&self) -> usize {
        self.addresses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub fn number_of_chunks(&self) -> usize {
        self.len().div_ceil(self.chunk_size())
    }

    pub fn iter(&self) -> &[UCInstructionAddress] {
        &self.addresses
    }
}
