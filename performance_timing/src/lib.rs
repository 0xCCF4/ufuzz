#![cfg_attr(not(test), no_std)]

mod measurements;

extern crate alloc;

mod arch {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    mod x86;
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    pub use x86::*;
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
#[derive(Copy, Clone)]
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
            exclusive: None,
        }
    }

    pub fn begin_exclusive(name: &'static str) -> Self {
        Self {
            name,
            start: instance().now(),
            exclusive: Some(mm_instance().borrow_mut().register_exclusive_measurement()),
        }
    }

    pub fn stop(mut self) -> Duration {
        self.__drop()
    }

    fn __drop(&mut self) -> Duration {
        let now = instance().now();
        let mut duration = now - self.start;
        if let Some(exclusive) = self.exclusive.take().map(ExclusiveMeasurementGuard::stop) {
            duration.0 = duration.0.saturating_sub(exclusive.0);
        }
        mm_instance()
            .borrow_mut()
            .register_data_point(self.name, duration);
        duration
    }
}

impl Drop for TimeMeasurement {
    fn drop(&mut self) {
        let _ = self.__drop();
    }
}
