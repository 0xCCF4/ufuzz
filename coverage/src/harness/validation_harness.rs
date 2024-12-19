use alloc::vec::Vec;
use core::arch::asm;
use data_types::addresses::UCInstructionAddress;
use crate::harness::coverage_harness::{CoverageError, CoverageHarness, ExecutionResultEntry};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct State<R: PartialEq + Eq> {
    result: R,
    rax: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}

impl<R: PartialEq + Eq> State<R> {
    pub fn is_state_equal(&self, other: &State<R>) -> bool {
        self == other
    }
    pub fn is_result_equal(&self, other: &State<R>) -> bool {
        self.result == other.result
    }
    pub fn is_state_equal_exclude_result(&self, other: &State<R>) -> bool {
        self.rax == other.rax && self.rcx == other.rcx && self.rdx == other.rdx && self.rsi == other.rsi && self.rdi == other.rdi &&
            self.r8 == other.r8 && self.r9 == other.r9 && self.r10 == other.r10 && self.r11 == other.r11 && self.r12 == other.r12 &&
            self.r13 == other.r13 && self.r14 == other.r14 && self.r15 == other.r15
    }
}

struct StateCapturer;

impl StateCapturer {
    #[inline(always)]
    fn prepare_state() -> Self {
        unsafe {
            asm! {
            "xor rax, rax",
            "xor rcx, rcx",
            "xor rdx, rdx",
            "xor rsi, rsi",
            "xor rdi, rdi",
            "xor r8, r8",
            "xor r9, r9",
            "xor r10, r10",
            "xor r11, r11",
            "xor r12, r12",
            "xor r13, r13",
            "xor r14, r14",
            "xor r15, r15",
            out("rax") _,
            // out("rbx") _,
            out("rcx") _,
            out("rdx") _,
            out("rsi") _,
            out("rdi") _,
            out("r8") _,
            out("r9") _,
            out("r10") _,
            out("r11") _,
            out("r12") _,
            out("r13") _,
            out("r14") _,
            out("r15") _,
            // out("rsp") _,
            // out("rbp") _,
            options(nostack),
            }
        }

        StateCapturer
    }

    #[inline(always)]
    pub fn capture<R: PartialEq + Eq>(self, result: R) -> State<R> {
        let rax: u64;
        let rcx: u64;
        let rdx: u64;
        let rsi: u64;
        let rdi: u64;
        let r8: u64;
        let r9: u64;
        let r10: u64;
        let r11: u64;
        let r12: u64;
        let r13: u64;
        let r14: u64;
        let r15: u64;
        unsafe {
            asm! {
            "nop",
            out("rax") rax,
            // out("rbx") _,
            out("rcx") rcx,
            out("rdx") rdx,
            out("rsi") rsi,
            out("rdi") rdi,
            out("r8") r8,
            out("r9") r9,
            out("r10") r10,
            out("r11") r11,
            out("r12") r12,
            out("r13") r13,
            out("r14") r14,
            out("r15") r15,
            // out("rsp") _,
            // out("rbp") _,
            options(nostack),
            }
        }
        State {
            rax, rcx, rdx, rsi, rdi, r8, r9, r10, r11, r12, r13, r14, r15, result
        }
    }
}

#[derive(Debug, Clone)]
pub enum ValidationError<R: PartialEq + Eq> {
    StateMismatch {
        state_with_coverage_collection: State<R>,
        state_without_coverage_collection: State<R>,
        coverage: Vec<ExecutionResultEntry>,
    },
    ExecutionFailure(CoverageError)
}

impl<R: PartialEq + Eq> From<CoverageError> for ValidationError<R> {
    fn from(error: CoverageError) -> Self {
        ValidationError::ExecutionFailure(error)
    }
}

#[derive(Debug, Clone)]
pub struct ValidationResult<R: PartialEq + Eq> {
    pub result: R,
    pub hooks: Vec<ExecutionResultEntry>
}

/// runs a function using coverage collection, then without and compare results for validity
pub struct ValidationHarness<'a, 'b, 'c> {
    coverage_harness: CoverageHarness<'a, 'b, 'c>
}

impl<'a, 'b, 'c> ValidationHarness<'a, 'b, 'c> {
    pub fn new(coverage_harness: CoverageHarness<'a,'b,'c>) -> Self {
        ValidationHarness {
            coverage_harness
        }
    }

    pub fn coverage_harness(&self) -> &CoverageHarness<'a, 'b, 'c> {
        &self.coverage_harness
    }

    pub fn into_inner(self) -> CoverageHarness<'a, 'b, 'c> {
        self.coverage_harness
    }

    pub fn execute<FuncParam, FuncResult: PartialEq + Eq, F: Copy + Fn(&FuncParam) -> FuncResult>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
        param: FuncParam,
    ) -> Result<ValidationResult<FuncResult>, ValidationError<FuncResult>> {
        let wrapped_func = move |param: &FuncParam| {
            let state = StateCapturer::prepare_state();

            let result = func(param);

            state.capture(result)
        };


        let coverage_result = self.coverage_harness.execute(hooks, wrapped_func, &param)?;
        let no_coverage_result = wrapped_func(&param);

        if no_coverage_result.is_state_equal(&coverage_result.result) {
            Ok(ValidationResult {
                result: coverage_result.result.result,
                hooks: coverage_result.hooks
            })
        } else {
            // todo address problems with w.g. calling rdrand()
            Err(ValidationError::StateMismatch {
                state_with_coverage_collection: coverage_result.result,
                coverage: coverage_result.hooks,
                state_without_coverage_collection: no_coverage_result
            })
        }
    }
}