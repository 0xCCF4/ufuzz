use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::arch::asm;
use custom_processing_unit::{
    CustomProcessingUnit, apply_hook_patch_func, apply_patch, hook, ms_seqw_write,
};
use data_types::addresses::MSRAMHookIndex;
use log::{error, trace};
use poc_data::f1_microspectre::{FenceType, Payload, ResultingData, TestResult};
use ucode_compiler_dynamic::sequence_word::{SequenceWord, SequenceWordSync};

pub fn execute(payload: Payload) -> ResultingData {
    trace!("[F1]: {payload:?}");

    match payload {
        Payload::Reset => reset()
            .map(ResultingData::Error)
            .unwrap_or(ResultingData::Ok),
        Payload::Sync => {
            sync(true);
            ResultingData::Ok
        }
        Payload::NoSync => {
            sync(false);
            ResultingData::Ok
        }
        Payload::Test => ResultingData::TestResult(rdrand()),
        Payload::Execute(fence_type) => {
            ResultingData::CacheTimings(run_scenario(fence_type).to_vec())
        }
    }
}

fn sync(enabled: bool) {
    let mut seqw = SequenceWord::new();
    if enabled {
        seqw.set_sync(0, SequenceWordSync::SYNCFULL);
    }
    ms_seqw_write(
        patch::LABEL_SPECULATIVE_WINDOW,
        seqw.assemble().unwrap() as usize,
    )
}

#[repr(C, align(4096))]
pub struct Page([u8; 4096]);

fn run_scenario(fence_type: FenceType) -> [u64; 2] {
    match fence_type {
        FenceType::None => scenario::<0>(),
        FenceType::CPUID => scenario::<1>(),
        FenceType::MICRO => scenario::<2>(),
    }
}

fn scenario<const VERSION: u64>() -> [u64; 2] {
    // allocate two memory pages on the heap
    let mut trigger_page = Box::new(Page([0x01; 4096]));
    let compare_page = Box::new(Page([0x01; 4096]));

    let before_compare: u64;
    let after_compare: u64;
    let before_trigger: u64;
    let after_trigger: u64;
    //let ebx_save: u64;

    unsafe {
        asm!(
        "wbinvd", // invalidate caches
        //"xchg {ebx_save}, ebx", // save ebx since overwritten by cpuid
        "mfence", // serialize load/store
        "cpuid",  // serialize instructions
        out("eax") _,
        out("ecx") _,
        out("edx") _,
        //ebx_save = out(reg) ebx_save,
        );
    }

    if VERSION == 0 {
        unsafe {
            asm!(

            //////////////// PAYLOAD
            "rdrand ebx", // execute payload (input r9, output r8)
            "mov r10, [r8]", // loads value from r8
            //////////////// PAYLOAD
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            out("r8") _,
            out("r10") _,
            in("r9") (trigger_page.as_mut() as *mut Page) as usize,
            );
        }
    }
    if VERSION == 1 {
        unsafe {
            asm!(

            //////////////// PAYLOAD
            "rdrand ebx", // execute payload (input r9, output r8)
            "cpuid", // according to intel manual fence instruction execution
            "mov r10, [r8]", // loads value from r8
            //////////////// PAYLOAD
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            out("r8") _,
            out("r10") _,
            in("r9") (trigger_page.as_mut() as *mut Page) as usize,
            );
        }
    }
    if VERSION == 2 {
        unsafe {
            asm!(

            //////////////// PAYLOAD
            "rdrand ebx", // execute payload (input r9, output r8)
            "rdseed ebx", // minimal possible implementation in microcode
            "mov r10, [r8]", // loads value from r8
            //////////////// PAYLOAD
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            out("r8") _,
            out("r10") _,
            in("r9") (trigger_page.as_mut() as *mut Page) as usize,
            );
        }
    }

    unsafe {
        asm!(
        // now capture access times
        "rdtsc", // save current timestamp to {before_trigger}
        "mov {before_trigger:e}, edx",
        "shl {before_trigger}, 32",
        "or {before_trigger:e}, eax",
        "cpuid", // synchronize instructions

        "mov r10, [{trigger_page}]",
        "lfence",
        "rdtsc", // save current timestamp to {after_trigger}
        "mov {after_trigger:e}, edx",
        "shl {after_trigger}, 32",
        "or {after_trigger:e}, eax",
        "cpuid", // synchronize instructions

        "rdtsc", // save current timestamp to {before_compare}
        "mov {before_compare:e}, edx",
        "shl {before_compare}, 32",
        "or {before_compare:e}, eax",
        "cpuid", // synchronize instructions

        "mov r10, [{compare_page}]",
        "lfence",
        "rdtsc", // save current timestamp to {after_compare}
        "mov {after_compare:e}, edx",
        "shl {after_compare}, 32",
        "or {after_compare:e}, eax",
        "cpuid", // synchronize instructions

        //"xchg {ebx_save}, ebx",
        out("eax") _,
        //ebx_save = in(reg) ebx_save,
        out("ecx") _,
        out("edx") _,
        out("r10") _,
        after_compare = out(reg) after_compare,
        before_compare = out(reg) before_compare,
        after_trigger = out(reg) after_trigger,
        before_trigger = out(reg) before_trigger,
        trigger_page = in(reg) (trigger_page.as_ref() as *const Page) as usize,
        compare_page = in(reg) (compare_page.as_ref() as *const Page) as usize,
        );
    }

    let timing_trigger_page = after_trigger.overflowing_sub(before_trigger).0;
    let timing_compare_page = after_compare.overflowing_sub(before_compare).0;

    [timing_trigger_page, timing_compare_page]
}

fn reset() -> Option<String> {
    let mut cpu = match CustomProcessingUnit::new() {
        Ok(cpu) => cpu,
        Err(e) => {
            error!("Failed to initiate program {:?}", e);
            return Some(format!("Failed to initiate program {:?}", e));
        }
    };
    if let Err(e) = cpu.init() {
        error!("Failed to initiate program {:?}", e);
        return Some(format!("Failed to initiate program {:?}", e));
    }

    if let Err(e) = cpu.zero_hooks() {
        error!("Failed to zero hooks {:?}", e);
        return Some(format!("Failed to zero hooks {:?}", e));
    }

    if let Err(err) = apply_patch(&patch::PATCH) {
        error!("Failed to apply patch {:?}", err);
        return Some(format!("Failed to apply patch {:?}", err));
    }

    drop(cpu);

    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        ucode_dump::dump::cpu_000506CA::RDRAND_XLAT,
        patch::LABEL_POC,
        true,
    ) {
        error!("Failed to apply patch {:?}", err);
        return Some(format!("Failed to apply patch {:?}", err));
    }
    if let Err(err) = hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO + 1,
        ucode_dump::dump::cpu_000506CA::RDSEED_XLAT,
        patch::LABEL_SYNCFULL,
        true,
    ) {
        error!("Failed to apply patch {:?}", err);
        return Some(format!("Failed to apply patch {:?}", err));
    }

    None
}

mod patch {
    use ucode_compiler_derive::patch;

    patch!(
        .org 0x7c00

        <POC>
        rcx := ZEROEXT_DSZ64(0xdede) # proof of run

        NOPB
        tmp2 := ZEROEXT_DSZ64(0xabab)
        tmp0 := ZEROEXT_DSZ64(0x1000)
        tmp1 := LDPPHYS_DSZ32_ASZ16_SC1(tmp0)
        tmp0 := SUB_DSZ64(tmp0, tmp1)

        # always jumps to taken
        UJMPCC_DIRECT_NOTTAKEN_CONDNZ(tmp0, <taken>)

        NOPB

        # CPU predicts execution continues here
        <speculative_window>
        NOP
        r8 := ZEROEXT_DSZ64(r9)
        NOP

        # if branch was actually not-taken rdx is now 0xdead
        rdx := ZEROEXT_DSZ64(0xdead) SEQW UEND0

        NOPB

        <taken>
        rax := ZEROEXT_DSZ64(0xabab) # rax is set to 0xabab to proof execution continued here
        <syncfull>
        unk_256() !m1 SEQW SYNCFULL, UEND0

        NOPB
        <minimal>
        NOP SEQW UEND0
    );
}

fn rdrand() -> TestResult {
    let before: u64;
    let sanity_check: u64;
    let completion_check: u64;
    unsafe {
        asm! {
        "rdrand r9",
        out("r9") _,
        out("rcx") before,
        out("rdx") sanity_check,
        out("rax") completion_check,
        options(nostack),
        }
    }
    if before != 0xdede {
        return TestResult::MarkerMissing;
    }
    if sanity_check == 0xdead {
        return TestResult::BranchWasNotTakenWhoops;
    }
    if completion_check != 0xabab {
        return TestResult::MarkerMissing;
    }
    TestResult::Ok
}

/*
asm!(
            "wbinvd", // invalidate caches
            "xchg {ebx_save}, ebx", // save ebx since overwritten by cpuid
            "mfence", // serialize load/store
            "cpuid",  // serialize instructions

            //////////////// PAYLOAD
            "rdrand ebx", // execute payload (input r9, output r8)
            "lfence", // now wait for
            "cpuid",  // sync
            "mov r10, [r8]", // loads value from r8
            //////////////// PAYLOAD

            "lfence", // let load complete

            // now capture access times
            "rdtsc", // save current timestamp to {before_trigger}
            "mov {before_trigger:e}, edx",
            "shl {before_trigger}, 32",
            "or {before_trigger:e}, eax",
            "cpuid", // synchronize instructions

            "mov r10, [{trigger_page}]",
            "lfence",

            "rdtsc", // save current timestamp to {after_trigger}
            "mov {after_trigger:e}, edx",
            "shl {after_trigger}, 32",
            "or {after_trigger:e}, eax",
            "cpuid", // synchronize instructions

            "rdtsc", // save current timestamp to {before_compare}
            "mov {before_compare:e}, edx",
            "shl {before_compare}, 32",
            "or {before_compare:e}, eax",
            "cpuid", // synchronize instructions

            "mov r10, [{compare_page}]",
            "lfence",

            "rdtsc", // save current timestamp to {after_compare}
            "mov {after_compare:e}, edx",
            "shl {after_compare}, 32",
            "or {after_compare:e}, eax",
            "cpuid", // synchronize instructions

            "xchg {ebx_save}, ebx",

            out("eax") _,
            ebx_save = out(reg) _,
            out("ecx") _,
            out("edx") _,

            in("r9") (trigger_page.as_mut() as *mut Page) as usize,
            out("r10") _,

            after_compare = out(reg) after_compare,
            before_compare = out(reg) before_compare,
            after_trigger = out(reg) after_trigger,
            before_trigger = out(reg) before_trigger,

            trigger_page = in(reg) (trigger_page.as_ref() as *const Page) as usize,
            compare_page = in(reg) (compare_page.as_ref() as *const Page) as usize,
            );
 */
