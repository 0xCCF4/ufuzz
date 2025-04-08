#![cfg_attr(all(not(test), not(target_arch = "aarch64")), no_std)]

pub mod measurements;

extern crate alloc;

pub use performance_timing_macros::*;

mod arch {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    mod x86;
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    pub use x86::*;
    #[cfg(any(target_arch = "aarch64"))]
    mod aarch64;
    #[cfg(any(target_arch = "aarch64"))]
    pub use aarch64::*;
}

use crate::measurements::{mm_instance, ExclusiveMeasurementGuard};
pub use arch::*;
use core::ops::{Add, AddAssign, Sub};
use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Copy, Clone, PartialEq, Hash, Debug, Default, Eq)]
pub enum Availability {
    /// Global invariant clock is available
    Full,
    /// Global clock available but not invariant
    Partial,
    /// No global clock available
    #[default]
    None,
}

#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug)]
pub struct Instant(TimeStamp);

impl Instant {
    pub fn new(time: TimeStamp) -> Self {
        Self(time)
    }
}

impl From<TimeStamp> for Instant {
    fn from(value: TimeStamp) -> Self {
        Self::new(value)
    }
}

impl Sub for Instant {
    type Output = Duration;
    fn sub(self, rhs: Self) -> Self::Output {
        Duration(self.0.wrapping_sub(rhs.0))
    }
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct Duration(TimeStamp);

impl Add for Duration {
    type Output = Duration;
    fn add(self, rhs: Self) -> Self::Output {
        Duration(self.0.saturating_add(rhs.0))
    }
}

impl AddAssign for Duration {
    fn add_assign(&mut self, rhs: Self) {
        self.0 = self.0.wrapping_add(rhs.0);
    }
}

impl From<Duration> for f64 {
    fn from(value: Duration) -> Self {
        value.0 as f64
    }
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut INSTANCE: Option<TimeKeeper> = None;

#[allow(static_mut_refs)]
pub fn instance() -> &'static TimeKeeper {
    if !INITIALIZED.load(Ordering::Relaxed) {
        panic!("Not initialized yet!");
    } else {
        unsafe { INSTANCE.as_ref().unwrap() }
    }
}

pub struct TimeMeasurement {
    pub name: &'static str,
    pub start: Instant,
    pub exclusive: Option<ExclusiveMeasurementGuard>,
}

impl TimeMeasurement {
    pub fn begin(name: &'static str) -> Self {
        Self {
            name,
            start: instance().now(),
            exclusive: Some(mm_instance().borrow_mut().register_exclusive_measurement()),
        }
    }

    pub fn stop_exclusive(mut self) -> Duration {
        self.__drop().1
    }

    pub fn stop_total(mut self) -> Duration {
        self.__drop().0
    }

    pub fn stop(mut self) -> (Duration, Duration) {
        self.__drop()
    }

    fn __drop(&mut self) -> (Duration, Duration) {
        let now = instance().now();
        let total_duration = now - self.start;
        let mut exclusive_duration = total_duration;
        if let Some(exclusive) = self.exclusive.take().map(ExclusiveMeasurementGuard::stop) {
            exclusive_duration.0 = exclusive_duration.0.saturating_sub(exclusive.0);
        }
        mm_instance().borrow_mut().register_data_point(
            self.name,
            total_duration,
            exclusive_duration,
        );

        (total_duration, exclusive_duration)
    }
}

impl Drop for TimeMeasurement {
    fn drop(&mut self) {
        let _ = self.__drop();
    }
}
