//! # Performance Timing
//! 
//! This crate provides tools for measuring and analyzing performance timing
//! in both std and no_std environments using the `rdtsc` instruction.
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

use crate::measurements::{mm_instance, ExclusiveMeasurementGuard, MeasureStackGuard};
pub use arch::*;
use core::ops::{Add, AddAssign, Sub};
use core::sync::atomic::{AtomicBool, Ordering};

/// Availability level of timing measurement functionality
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

/// A point in time
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug)]
pub struct Instant(TimeStamp);

impl Instant {
    /// Create a new instant from a timestamp
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

/// A duration between two instants
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

/// Check if timing functionality is available
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

/// Get the global timekeeper instance
#[allow(static_mut_refs)]
pub fn instance() -> &'static TimeKeeper {
    if !INITIALIZED.load(Ordering::Relaxed) {
        panic!("Not initialized yet!");
    } else {
        unsafe { INSTANCE.as_ref().unwrap() }
    }
}

/// A measurement of execution time
pub struct TimeMeasurement {
    /// Name of the measurement
    pub name: &'static str,
    /// Start time
    pub start: Instant,
    /// Guard for exclusive time measurement
    pub exclusive: Option<ExclusiveMeasurementGuard>,
    /// Guard for stack-based measurement
    pub stack_guard: Option<MeasureStackGuard>,
}

impl TimeMeasurement {
    /// Begin a new time measurement
    pub fn begin(name: &'static str) -> Self {
        let mut guard = mm_instance().borrow_mut();
        Self {
            name,
            start: instance().now(),
            exclusive: Some(guard.register_exclusive_measurement(name)),
            stack_guard: Some(guard.begin_stack_frame(name)),
        }
    }

    /// Stop measurement and return exclusive time
    pub fn stop_exclusive(mut self) -> Duration {
        self.__drop().1
    }

    /// Stop measurement and return total time
    pub fn stop_total(mut self) -> Duration {
        self.__drop().0
    }

    /// Stop measurement and return both total and exclusive time
    pub fn stop(mut self) -> (Duration, Duration) {
        self.__drop()
    }

    fn __drop(&mut self) -> (Duration, Duration) {
        drop(self.stack_guard.take());
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
