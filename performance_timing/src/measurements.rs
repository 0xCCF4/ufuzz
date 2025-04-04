use crate::{instance, Duration};
use alloc::collections::BTreeMap;
use core::cell::RefCell;
use core::fmt::Display;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use serde::{Deserialize, Serialize};

static MM_INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut MM_INSTANCE: Option<RefCell<MeasurementManager>> = None;

pub fn mm_instance() -> &'static RefCell<MeasurementManager> {
    if !MM_INITIALIZED.load(Ordering::Relaxed) {
        panic!("Not initialized yet!");
    } else {
        unsafe { MM_INSTANCE.as_ref().unwrap() }
    }
}

pub fn mm_initialize() -> &'static RefCell<MeasurementManager> {
    if MM_INITIALIZED.load(Ordering::Relaxed) {
        return mm_instance();
    }
    unsafe {
        MM_INSTANCE = Some(RefCell::new(MeasurementManager::new()));
    }
    MM_INITIALIZED.store(true, Ordering::Relaxed);
    mm_instance()
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MeasureValues {
    pub cumulative_average: f64,
    pub cumulative_sum_of_squares: f64,
    pub number_of_measurements: u64,
}

impl MeasureValues {
    pub fn variance(&self) -> f64 {
        if self.number_of_measurements == 0 {
            return 0.0;
        }
        self.cumulative_sum_of_squares / (self.number_of_measurements as f64)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MeasurementManager {
    pub data: BTreeMap<&'static str, MeasureValues>,
    #[serde(skip)]
    pub exclusive_time_keeping: BTreeMap<u64, Duration>,
}

impl MeasurementManager {
    pub fn new() -> Self {
        Self {
            data: BTreeMap::new(),
            exclusive_time_keeping: BTreeMap::new(),
        }
    }

    pub fn register_data_point(&mut self, name: &'static str, duration: Duration) {
        let x = instance().duration_to_seconds(duration);
        let data = self.data.entry(name).or_default();
        if data.number_of_measurements == 0 {
            data.cumulative_average = x;
            data.cumulative_sum_of_squares = 0.0;
            data.number_of_measurements = 1;
        } else if data.number_of_measurements == u64::MAX {
            return;
        } else {
            data.number_of_measurements += 1;
            data.cumulative_average +=
                (x - data.cumulative_average) / (data.number_of_measurements as f64);
            data.cumulative_sum_of_squares +=
                (x - data.cumulative_average) * (x - data.cumulative_average);
        }

        self.exclusive_time_keeping
            .iter_mut()
            .for_each(|(_, v)| *v += duration);
    }

    pub fn reset(&mut self) {
        self.data.clear();
    }

    fn format_duration(duration: f64) -> (f64, &'static str) {
        if duration >= 1.0 {
            (duration, " s")
        } else if duration >= 1e-3 {
            (duration * 1e3, "ms")
        } else if duration >= 1e-6 {
            (duration * 1e6, "µs")
        } else {
            (duration * 1e9, "ns")
        }
    }

    pub fn register_exclusive_measurement(&mut self) -> ExclusiveMeasurementGuard {
        let guard = ExclusiveMeasurementGuard::begin();
        self.exclusive_time_keeping.insert(guard.index, Duration(0));
        guard
    }

    fn drop_exclusive_measurement(&mut self, index: u64) -> Duration {
        if let Some(duration) = self.exclusive_time_keeping.remove(&index) {
            duration
        } else {
            Duration(0)
        }
    }
}

impl Display for MeasurementManager {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "{:<20} | {:<20} | {:<20} | {:<20}",
            "Name", "Cumulative Average", "Variance", "Number of Measurements"
        )?;
        for (k, v) in mm_instance().borrow().data.iter() {
            let (avg, avg_unit) = Self::format_duration(v.cumulative_average);
            let (var, var_unit) = Self::format_duration(v.variance());
            writeln!(
                f,
                "{:<20} | {:<17.3} {} | {:<17.3} {} | {:<20.1e}",
                k, avg, avg_unit, var, var_unit, v.number_of_measurements as f64
            )?;
        }

        Ok(())
    }
}

#[repr(transparent)]
pub struct ExclusiveMeasurementGuard {
    index: u64,
}

static mut TIME_MEASUREMENT_INDEX: AtomicU64 = AtomicU64::new(0);

impl ExclusiveMeasurementGuard {
    pub fn begin() -> Self {
        let index = unsafe { TIME_MEASUREMENT_INDEX.fetch_add(1, Ordering::SeqCst) };
        Self { index }
    }

    pub fn stop(self) -> Duration {
        self.__drop()
    }

    fn __drop(&self) -> Duration {
        mm_instance()
            .borrow_mut()
            .drop_exclusive_measurement(self.index)
    }
}

impl Drop for ExclusiveMeasurementGuard {
    fn drop(&mut self) {
        let _ = self.__drop();
    }
}
