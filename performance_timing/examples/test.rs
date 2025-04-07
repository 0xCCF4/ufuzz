#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]

use performance_timing::initialize;
use performance_timing::measurements::mm_instance;
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
    let _ = initialize(2_699_000_000f64);

    for _ in 0..1000 {
        test_time_measurement();
    }

    println!("{}", mm_instance().borrow());
}
