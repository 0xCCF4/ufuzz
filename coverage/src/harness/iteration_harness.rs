use crate::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use data_types::addresses::UCInstructionAddress;

pub struct IterationRun<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult> {
    chunk_size: usize,
    next_chunk_index: usize,
    address_iterator: &'a HookableAddressIterator,
    function: F,
}

impl<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>
    IterationRun<'a, FuncResult, F>
{
    pub fn new(address_iterator: &'a HookableAddressIterator, function: F, chunk_size: usize) -> Self {
        IterationRun {
            next_chunk_index: 0,
            address_iterator,
            function,
            chunk_size,
        }
    }

    pub fn number_of_chunks(&self) -> usize {
        self.address_iterator.len().div_ceil(self.chunk_size())
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

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

pub struct IterationHarness {
    address_iterator: HookableAddressIterator,
}

impl IterationHarness {
    pub fn new(address_iterator: HookableAddressIterator) -> Self {
        IterationHarness { address_iterator }
    }

    pub fn execute_for_all_addresses<FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>(
        &self,
        func: F,
    ) -> IterationRun<FuncResult, F> {
        IterationRun::new(&self.address_iterator, func, self.address_iterator.chunk_size())
    }

    pub fn execute_for_all_addresses_with_size<FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>(
        &self,
        chunk_size: usize,
        func: F,
    ) -> IterationRun<FuncResult, F> {
        IterationRun::new(&self.address_iterator, func, self.address_iterator.chunk_size().min(chunk_size))
    }
}
