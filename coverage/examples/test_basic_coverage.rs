#![no_main]
#![no_std]

extern crate alloc;

use core::arch::asm;
use coverage::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use coverage::{interface_definition};
use custom_processing_unit::{
    lmfence, CustomProcessingUnit,
    FunctionResult,
};
use data_types::addresses::UCInstructionAddress;
use itertools::Itertools;
use log::info;
use uefi::prelude::*;
use uefi::{print, println};

#[entry]
unsafe fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");

    let cpu = match CustomProcessingUnit::new() {
        Ok(cpu) => cpu,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    if let Err(e) = cpu.init() {
        info!("Failed to initiate program {:?}", e);
        return Status::ABORTED;
    }

    if let Err(e) = cpu.zero_hooks() {
        info!("Failed to zero hooks {:?}", e);
        return Status::ABORTED;
    }

    let mut interface = match ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION) {
        Ok(interface) => interface,
        Err(e) => {
            info!("Failed to initiate program {:?}", e);
            return Status::ABORTED;
        }
    };
    let hooks = {
        let max_hooks = interface.description().max_number_of_hooks;

        let device_max_hooks = match cpu.current_glm_version {
            custom_processing_unit::GLM_OLD => 31,
            custom_processing_unit::GLM_NEW => 32,
            _ => 0,
        };

        max_hooks.min(device_max_hooks)
    };

    if hooks == 0 {
        info!("No hooks available");
        return Status::ABORTED;
    }

    interface.reset_coverage();

    interface.reset_coverage();

    let mut harness = CoverageHarness::new(&mut interface);
    harness.init();

    let mut count = 0;
    let mut covered = 0;

    for chunk in (0..0x7c00)
        .into_iter()
        .filter(|i| (i % 2) == 0 && (i % 4) < 3)
        .filter(|address| filter_blacklisted_instruction(*address))
        .chunks(hooks.min(1))
        .into_iter()
    {
        let addresses = chunk
            .map(|i| UCInstructionAddress::from_const(i))
            .collect_vec();

        if addresses.len() == 0 {
            break;
        }

        print!(
            "\r[{}]: ",
            &addresses.first().unwrap()
        );

        if let Err(e) = harness.execute(
            &addresses,
            |_| {
                for _ in 0..32 {
                    rdrand();
                }
            },
            (),
        ) {
            println!("Failed to execute harness: {:?}", e);
            continue;
        }

        count += addresses.len();

        if addresses.iter().any(|a| harness.covered(a)) {
            print!("Covered: ");
            for address in &addresses {
                if harness.covered(address) {
                    print!("{} ", address);
                    covered += 1;
                }
            }
            println!();
        }
    }

    println!("Covered: {}/{} {}%", covered, count, (covered as f64 / count as f64)*100.0);

    if let Err(err) = cpu.zero_hooks() {
        println!("Failed to zero hooks: {:?}", err);
    }

    println!("Goodbye!");
    Status::SUCCESS
}

#[rustfmt::skip]
fn filter_blacklisted_instruction(address: usize) -> bool {
    let blacklist = include!("../src/blacklist.txt");
    !blacklist.contains(&address)
    /*

    // todo: recheck and reduce possible?

    // Copied from custom processing unit

    const DUMP: [u64; 0x8000] = include!("ucode_dump.txt");

    fn get_opcode(uop: u64) -> u16 {
        ((uop >> 32) & 0xfff) as u16
    }

    assert_ne!(address % 4, 3, "Precondition failed. address % 4 == 3");
    assert!(address / 4 < DUMP.len(), "Precondition failed. address / 4 < dump.len(). Address out of bounds.");

    let opcode = get_opcode(DUMP[address]);
    if opcode == 0xfef { return false; } // LBSYNC
    
    // new GLM
    if address == 0x1544 { return false; }
    if address == 0x2280 { return false; }
    if address == 0x2282 { return false; }
    if address == 0x368c { return false; }
    if address == 0x36c2 { return false; }
    if address == 0x6004 { return false; }
    if address == 0x6016 { return false; }
    
    // ucode update instructions
    if address == 0x0010 { return false; } // SAVEUIP(0x01, U0352) m0=1 SEQW GOTO U0911
    if address == 0x0058 { return false; } // SAVEUIP( , 0x01, U0c79) m0=1 SEQW GOTO U06f1
    if address == 0x0138 { return false; } // LDZX_DSZ16_ASZ32_SC1(DS, r64base, r64idx, IMM_MACRO_ALIAS_DISPLACEMENT, mode=0x18) m0=1
    if address == 0x033c { return false; } // SYNCFULL-> UJMP( , U2e3d)
    if address == 0x03f2 { return false; } // r64dst:= ZEROEXT_DSZ32N(tmp0, r64dst) !m1 SEQW UEND0
    if address == 0x0492 { return false; } // tmp0:= unk_f3f(rsp) m0=1 m1=1
    if address == 0x09da { return false; } // AETTRACE( , 0x08, IMM_MACRO_ALIAS_INSTRUCTION) !m0
    if address == 0x09dc { return false; } // rsp:= ADD_DSZN(IMM_MACRO_ALIAS_DATASIZE, rsp) !m0,m1
    if address == 0x09de { return false; } // STAD_DSZN_ASZ32_SC1(tmp1,  , mode=0x18, tmp0) !m1 SEQW UEND0
    if address == 0x0a94 { return false; } // MOVETOCREG_DSZ64(tmp10, CORE_CR_CR0) m2=1
    if address == 0x0ba8 { return false; } // tmp1:= RDSEGFLD(SEG_V0, SEL+FLGS+LIM) SEQW GOTO U08ea
    if address == 0x0bc8 { return false; } // tmp0:= unk_206( , 0x00000001)
    if address == 0x0c74 { return false; } // LFNCEWAIT-> STADPPHYSTICKLE_DSZ64_ASZ64_SC1(tmp12, tmp9, tmp7)
    if address == 0x182c { return false; } // tmp1:= MOVE_DSZ64(tmp5) SEQW GOTO U2431
    if address == 0x281c { return false; } // BTUJB_DIRECT_NOTTAKEN(tmp0, 0x00000014, patch_runs_load_loop) !m2 SEQW GOTO U281a
    if address == 0x2a98 { return false; } // tmp2:= ZEROEXT_DSZ32(0x00000000) SEQW GOTO U43ae
    if address == 0x2ad8 { return false; } // tmp2:= LDPPHYS_DSZ16_ASZ32_SC4( , tmp8, 0x00000004, mode=0x0f) SEQW GOTO U3a14
    if address == 0x2b14 { return false; } // SAVEUIP( , 0x01, U21fe) !m0
    if address == 0x32cc { return false; } // SAVEUIP( , 0x01, U324d) !m0
    if address == 0x5794 { return false; } // tmp4:= SAVEUIP( , 0x01, U079d) !m0 SEQW GOTO U5cfc
    if address == 0x57fc { return false; } // mm7:= FMOV( , tmm1) !m0 SEQW GOTO uend
    if address == 0x5a0c { return false; } // tmp5:= LDPPHYSTICKLE_DSZ64_ASZ64_SC1(tmp1, tmp2) SEQW GOTO U3026
    if address == 0x5b24 { return false; } // tmp13:= MOVEFROMCREG_DSZ64( , 0x287, 32) !m1 SEQW GOTO U1b0c
    /*manual uend*/
    if address == 0x5b26 { return false; } // tmp8:= MOVEFROMCREG_DSZ64( , 0x0b1)
    if address == 0x5b28 { return false; } // BTUJNB_DIRECT_NOTTAKEN(tmp8, 0x00000005, U5b29) !m2 SEQW GOTO U2d21
    if address == 0x5b2a { return false; } // MOVETOCREG_DSZ64( , 0x00000000, 0x10a) !m2
    if address == 0x5b2c { return false; } // BTUJNB_DIRECT_NOTTAKEN(tmp5, 0x00000008, U2d0e) !m1
    if address == 0x5be4 { return false; } // SYNCFULL-> UJMP( , tmp7)
    if address == 0x5c9e { return false; } // tmpv2:= MOVEFROMCREG_DSZ64( , 0x529)
    if address == 0x5ca0 { return false; } // LFNCEMARK-> tmpv1:= READURAM( , 0x0052, 64)
    if address == 0x5ca2 { return false; } // tmpv0:= SUB_DSZ64(tmpv1, tmpv0)
    if address == 0x5ca4 { return false; } // tmpv0:= SELECTCC_DSZ32_CONDNZ(tmpv0, 0x00000001)
    if address == 0x5ca6 { return false; } // tmpv1:= BT_DSZ32(tmpv1, 0x00000007)
    /*---------------*/
    if address == 0x5d74 { return false; } // WRITEURAM(tmp1, 0x0070, 64) !m2 SEQW GOTO U35fd
    if address == 0x5e04 { return false; } // tmp3:= LDPPHYSTICKLE_DSZ8_ASZ64_SC1(tmp4, 0x00000080, mode=0x1c) SEQW GOTO U0c72
    if address == 0x5e20 { return false; } // BTUJB_DIRECT_NOTTAKEN(tmp2, 0x00000017, U590c) !m0,m2 SEQW GOTO U05fc
    if address == 0x5ed4 { return false; } // WRITEURAM(tmp4, 0x001f, 32) !m2 SEQW GOTO do_smm_vmexit
    if address == 0x6018 { return false; } // PORTOUT_DSZ8_ASZ16_SC1(tmp2,  , tmp1) SEQW GOTO U66d2
    if address == 0x6160 { return false; } // MOVETOCREG_OR_DSZ64(tmp1, tmp2, 0x104) SEQW GOTO U3230
    if address == 0x619c { return false; } // SYNCWAIT-> tmp14:= READURAM( , 0x0043, 64) SEQW GOTO U4ded
    if address == 0x621c { return false; } // tmp11:= READURAM( , 0x000f, 64) SEQW GOTO U3c98
    if address == 0x68ac { return false; } // tmp11:= ZEROEXT_DSZ32(0x00020101) SEQW GOTO U669a
    
    // // instruction that freezes the CPU AFTER tracing all
    if address == 0x208c { return false; } // tmp9:= AND_DSZ64(0x00000800, tmp9) SEQW GOTO U4b22
    
    
    // UDBGRD/UDBWR instructions
    if address == 0x4052 { return false; }
    if address == 0x4054 { return false; }
    if address == 0x4064 { return false; }
    if address == 0x4066 { return false; }
    if address == 0x4092 { return false; }
    
    // unknown reason why it crashes here (rdmsr)
    if address == 0x3ce0 { return false; }
    
    // faulty readmsr
    if address == 0x0ea0 { return false; }
    if address == 0x2684 { return false; }
    if address == 0x38c8 { return false; }
    if address == 0x3bfc { return false; }
    if address == 0x3d64 { return false; }
    if address == 0x3d88 { return false; }
    if address == 0x4d50 { return false; }
    
    // unknown reason why it crashes here (wrmsr(0x1b))
    if address == 0x008e { return false; }
    if address == 0x69d0 { return false; }
    
    // ud2
    if address == 0xdc0 { return false; }
    
    // int3
    if address == 0x3a2c { return false; } // LFNCEWAIT-> MOVETOCREG_DSZ64(tmpv0, 0x6c0)
    if address == 0x33e4 { return false; } // SYNCFULL-> MOVETOCREG_DSZ64(tmp1, 0x7f5) !m2
    if address == 0x3d34 { return false; } // tmp14:= SAVEUIP(0x01, U0664) !m0 SEQW GOTO U5d81
    if address == 0x3e70 { return false; } // NOP SEQW GOTO U1f9a
    if address == 0x605c { return false; } // GENARITHFLAGS(tmp0, tmp7) !m2 SEQW UEND
    
    // int1
    if address == 0x3e94 { return false; } // MOVETOCREG_DSZ64(tmp0, 0x070)
    
    // div
    // if address == 0x6c8 { return false; }
    if address == 0x6ca { return false; }
    if address == 0x6cc { return false; }
    
    
    // The next addresses in the black list where crashing if the match&patch was not
    // reinitialized at every iteration. Keep them for future reference
    // if address == 0x3c8 { return false; }
    // if address == 0x490 { return false; }
    // if address == 0x492 { return false; }
    // if address == 0x6c8 { return false; }
    // if address == 0x6ca { return false; }

    true

    */
}

fn rdrand() -> (bool, FunctionResult) {
    let mut result = FunctionResult::default();
    let flags: u8;
    lmfence();
    unsafe {
        asm! {
        "xchg {rbx_tmp}, rbx",
        "rdrand rax",
        "setc {flags}",
        "xchg {rbx_tmp}, rbx",
        inout("rax") 0usize => result.rax,
        rbx_tmp = inout(reg) 0usize => result.rbx,
        inout("rcx") 0usize => result.rcx,
        inout("rdx") 0usize => result.rdx,
        flags = out(reg_byte) flags,
        options(nostack),
        }
    }
    lmfence();
    (flags > 0, result)
}
