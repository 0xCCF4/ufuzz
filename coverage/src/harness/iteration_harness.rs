use crate::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use data_types::addresses::UCInstructionAddress;

pub struct IterationRun<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult> {
    next_chunk_index: usize,
    address_iterator: &'a HookableAddressIterator,
    function: F,
}

impl<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>
    IterationRun<'a, FuncResult, F>
{
    pub fn new(address_iterator: &'a HookableAddressIterator, function: F) -> Self {
        IterationRun {
            next_chunk_index: 0,
            address_iterator,
            function,
        }
    }
}

impl<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult> Iterator
    for IterationRun<'a, FuncResult, F>
{
    type Item = FuncResult;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_chunk_index >= self.address_iterator.number_of_chunks() {
            None
        } else {
            let start = self.next_chunk_index * self.address_iterator.chunk_size();
            let end = (self.next_chunk_index + 1) * self.address_iterator.chunk_size();
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

    pub fn execute<'a, FuncResult, F: FnMut(&[UCInstructionAddress]) -> FuncResult>(
        &'a self,
        func: F,
    ) -> IterationRun<'a, FuncResult, F> {
        IterationRun::new(&self.address_iterator, func)
    }
}
