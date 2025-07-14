#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]

use performance_timing::measurements::{mm_instance, MeasurementCollection};
use performance_timing::{initialize, instance};
use performance_timing_macros::track_time;

#[track_time("test")]
fn test_time_measurement() {
    #[track_time("test2")]
    {
        #[track_time("test3")]
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub struct Test {}

#[track_time]
impl Test {
    fn x(&self) {}
}

#[track_time]
impl Default for Test {
    fn default() -> Self {
        Test {}
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    let _ = initialize(2_699_000_000f64); // Our development machine
    #[cfg(target_arch = "aarch64")]
    let _ = initialize(54_000_000.0); // Rpi4

    println!("Now: {:?}", instance().now());

    for _ in 0..100 {
        test_time_measurement();
    }

    println!("Now: {:?}", instance().now());

    println!(
        "{}",
        MeasurementCollection::from(mm_instance().borrow().data.clone()).normalize()
    );
}
