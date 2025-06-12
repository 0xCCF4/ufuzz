//! This module provides functionality for iterating over microcode addresses that can be hooked
//! for coverage collection. It ensures proper spacing between hooks and validates hookability
//! of addresses.

use crate::harness::coverage_harness::modification_engine;
use crate::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use alloc::rc::Rc;
use alloc::vec::Vec;
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use ucode_dump::RomDump;

/// Iterator over hookable microcode addresses
/// 
/// This structure manages a list of addresses that can be hooked for coverage collection,
/// ensuring proper spacing between hooks.
#[derive(Debug, Clone)]
pub struct HookableAddressIterator {
    /// List of hookable addresses
    addresses: Rc<Vec<UCInstructionAddress>>,
    /// Size of chunks for hook grouping
    chunk_size: usize,
}

impl HookableAddressIterator {
    /// Constructs a new iterator over hookable addresses
    /// 
    /// # Arguments
    /// 
    /// * `rom` - Reference to the ROM dump
    /// * `modification_engine_settings` - Settings for the modification engine
    /// * `chunk_size` - Size of chunks for hook grouping
    /// * `filter` - Additional filter function for addresses
    /// 
    /// # Returns
    /// 
    /// A new iterator instance
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

    /// Gets the total number of hookable addresses
    pub fn len(&self) -> usize {
        self.addresses.len()
    }

    /// Checks if there are no hookable addresses
    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }

    /// Gets the chunk size used for hook grouping
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Gets the number of chunks based on the total addresses and chunk size
    pub fn number_of_chunks(&self) -> usize {
        self.len().div_ceil(self.chunk_size())
    }

    /// Gets a slice of all hookable addresses
    pub fn iter(&self) -> &[UCInstructionAddress] {
        &self.addresses
    }
}
