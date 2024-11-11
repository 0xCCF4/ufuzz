use custom_processing_unit::patches::func_ldat_read;
use custom_processing_unit::{
    apply_hook_patch_func, apply_patch, hook, labels, ms_hook_read, ms_hook_write,
    CustomProcessingUnit,
};
use data_types::addresses::MSRAMHookIndex;
use log::info;
use std::env;
use std::io::Write;

mod patches;

#[allow(dead_code)]
fn random_counter() {
    let cpu = CustomProcessingUnit::new().error_unwrap();

    info!("Initializing");

    cpu.init().error_unwrap();

    info!("Zero match and patch");

    cpu.zero_hooks().error_unwrap();

    let patch = crate::patches::rdrand_patch;

    info!("Patching");

    apply_patch(&patch);

    info!("Hooking");

    hook(
        apply_hook_patch_func(),
        MSRAMHookIndex::ZERO,
        labels::RDRAND_XLAT,
        patch.addr,
        true,
    )
    .error_unwrap();

    info!("Zero match and patch");

    cpu.zero_hooks().error_unwrap();

    for i in 0..63 {
        // only till 63
        ms_hook_write(MSRAMHookIndex::ZERO + i, i);
    }
    apply_patch(&func_ldat_read);
    for i in 0..64 {
        let _ = ms_hook_read(func_ldat_read.addr, MSRAMHookIndex::ZERO + i);
    }
}

fn main() {
    // setup logger
    env::set_var("RUST_LOG", "trace");
    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();

    info!("Hello world!");

    info!("Random counter test");
    random_counter();
}

trait ErrorUnwrap<T> {
    fn error_unwrap(self) -> T;
}

impl<T, E> ErrorUnwrap<T> for Result<T, E>
where
    E: core::fmt::Display,
{
    #[track_caller]
    fn error_unwrap(self) -> T {
        match self {
            Err(e) => panic!("Result unwrap error: {}", e),
            Ok(content) => content,
        }
    }
}

impl<T> ErrorUnwrap<T> for Option<T> {
    #[track_caller]
    fn error_unwrap(self) -> T {
        match self {
            None => panic!("Option unwrap error: None"),
            Some(content) => content,
        }
    }
}
