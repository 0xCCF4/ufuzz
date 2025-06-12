//! This module provides functionality for iterating over microcode addresses in chunks,
//! allowing systematic coverage collection across the entire address space.
//! It manages chunked execution of coverage collection functions over sets of addresses.

use crate::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use data_types::addresses::UCInstructionAddress;

/// A utility structure for iterating over chunks of hookable addresses
pub struct IterationRun<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult> {
    /// Size of each chunk of addresses
    chunk_size: usize,
    /// Index of the next chunk to process
    next_chunk_index: usize,
    /// Iterator over hookable addresses
    address_iterator: &'a HookableAddressIterator,
    /// Function to execute for each chunk
    function: F,
}

impl<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>
    IterationRun<'a, FuncResult, F>
{
    /// Creates a new iteration run
    /// 
    /// # Arguments
    /// 
    /// * `address_iterator` - Iterator over hookable addresses
    /// * `function` - Function to execute for each chunk
    /// * `chunk_size` - Size of each chunk
    pub fn new(
        address_iterator: &'a HookableAddressIterator,
        function: F,
        chunk_size: usize,
    ) -> Self {
        IterationRun {
            next_chunk_index: 0,
            address_iterator,
            function,
            chunk_size,
        }
    }

    /// Gets the total number of chunks to process
    pub fn number_of_chunks(&self) -> usize {
        self.address_iterator.len().div_ceil(self.chunk_size())
    }

    /// Gets the size of each chunk
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

/// Iterator implementation for IterationRun
/// 
/// Allows iterating over chunks of addresses, executing the provided function
/// for each chunk.
impl<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult> Iterator
    for IterationRun<'a, FuncResult, F>
{
    type Item = FuncResult;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_chunk_index >= self.number_of_chunks() {
            None
        } else {
            let start = self.next_chunk_index * self.chunk_size();
            let end = (self.next_chunk_index + 1) * self.chunk_size();
            self.next_chunk_index += 1;

            let hooks = &self.address_iterator.iter()[start..end.min(self.address_iterator.len())];

            Some((self.function)(hooks))
        }
    }
}

/// Main harness for iterating over all hookable addresses
/// 
/// This structure provides methods for executing functions over all hookable
/// addresses in chunks.
pub struct IterationHarness {
    /// Iterator over hookable addresses
    address_iterator: HookableAddressIterator,
}

impl IterationHarness {
    /// Creates a new iteration harness
    /// 
    /// # Arguments
    /// 
    /// * `address_iterator` - Iterator over hookable addresses
    pub fn new(address_iterator: HookableAddressIterator) -> Self {
        IterationHarness { address_iterator }
    }

    /// Executes a function for all addresses using the default chunk size
    /// 
    /// # Arguments
    /// 
    /// * `func` - Function to execute for each chunk of addresses
    /// 
    /// # Returns
    /// 
    /// An reference to an iterator that can be used to process all chunks
    pub fn execute_for_all_addresses<
        FuncResult,
        F: FnMut(&[UCInstructionAddress]) -> FuncResult,
    >(
        &self,
        func: F,
    ) -> IterationRun<FuncResult, F> {
        IterationRun::new(
            &self.address_iterator,
            func,
            self.address_iterator.chunk_size(),
        )
    }

    /// Executes a function for all addresses using a custom chunk size
    /// 
    /// # Arguments
    /// 
    /// * `chunk_size` - Maximum size of each chunk
    /// * `func` - Function to execute for each chunk of addresses
    /// 
    /// # Returns
    /// 
    /// An reference to an iterator that can be used to process all chunks
    pub fn execute_for_all_addresses_with_size<
        FuncResult,
        F: FnMut(&[UCInstructionAddress]) -> FuncResult,
    >(
        &self,
        chunk_size: usize,
        func: F,
    ) -> IterationRun<FuncResult, F> {
        IterationRun::new(
            &self.address_iterator,
            func,
            self.address_iterator.chunk_size().min(chunk_size),
        )
    }
}
