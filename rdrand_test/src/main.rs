use std::env;
use custom_processing_unit::{
    labels,
    CustomProcessingUnit,
};
use data_types::addresses::{MSRAMHookAddress};
use log::info;

mod patches;

#[allow(dead_code)]
fn random_counter() {
    let cpu = CustomProcessingUnit::new().error_unwrap();

    info!("Initializing");

    cpu.init();

    info!("Zero match and patch");

    cpu.zero_match_and_patch().error_unwrap();

    let patch = crate::patches::rdrand_patch;

    info!("Patching");

    cpu.patch(&patch);

    info!("Hooking");

    cpu.hook(MSRAMHookAddress::ZERO, labels::RDRAND_XLAT, patch.addr)
        .error_unwrap();

    info!("Zero match and patch");

    cpu.zero_match_and_patch().error_unwrap();
}

fn main() {
    // setup logger
    env::set_var("RUST_LOG", "trace");
    env_logger::init();

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
