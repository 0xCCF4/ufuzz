use crate::{instance, Duration};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::{Display, Formatter};
use core::ops::AddAssign;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use num_traits::{AsPrimitive, SaturatingAdd};
use serde::{Deserialize, Serialize};

static MM_INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut MM_INSTANCE: Option<RefCell<MeasurementManager>> = None;

#[allow(static_mut_refs)]
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
pub struct MeasureValues<T> {
    pub exclusive_cumulative_average: f64,
    pub exclusive_cumulative_sum_of_squares: f64,
    pub exclusive_time: T,
    pub total_cumulative_average: f64,
    pub total_cumulative_sum_of_squares: f64,
    pub total_time: T,
    pub number_of_measurements: u64,
}

impl From<&MeasureValues<u64>> for MeasureValues<f64> {
    fn from(value: &MeasureValues<u64>) -> Self {
        Self {
            exclusive_cumulative_average: value.exclusive_cumulative_average,
            exclusive_cumulative_sum_of_squares: value.exclusive_cumulative_sum_of_squares,
            total_cumulative_average: value.total_cumulative_average,
            total_cumulative_sum_of_squares: value.total_cumulative_sum_of_squares,
            total_time: instance().duration_to_seconds(value.total_time as f64),
            exclusive_time: instance().duration_to_seconds(value.exclusive_time as f64),
            number_of_measurements: value.number_of_measurements,
        }
    }
}

impl<T: AsPrimitive<f64> + Copy> MeasureValues<T> {
    pub fn variance(&self) -> f64 {
        if self.number_of_measurements == 0 {
            return 0.0;
        }
        self.exclusive_cumulative_sum_of_squares / (self.number_of_measurements as f64)
    }
    pub fn total_time(&self, frequency: f64) -> f64 {
        self.total_time.as_() / frequency
    }
}

pub type MeasurementData<T> = BTreeMap<String, MeasureValues<T>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeasurementCollection<T> {
    pub data: Vec<MeasurementData<T>>,
}

impl MeasurementCollection<u64> {
    pub fn normalize(&self) -> MeasurementCollection<f64> {
        let mut result = MeasurementCollection::default();
        for entry in self.data.iter() {
            let mut new_entry = BTreeMap::new();
            for (k, v) in entry.iter() {
                new_entry.insert(k.clone(), v.into());
            }
            result.data.push(new_entry);
        }
        result
    }
}

impl<T> From<BTreeMap<String, MeasureValues<T>>> for MeasurementCollection<T> {
    fn from(value: BTreeMap<String, MeasureValues<T>>) -> Self {
        Self {
            data: vec![value],
        }
    }
}

pub trait SaturationFloatAdd {
    fn sat_add(&self, other: &Self) -> Self;
}

impl SaturationFloatAdd for f64 {
    fn sat_add(&self, other: &Self) -> Self {
        self + other
    }
}

impl SaturationFloatAdd for u64 {
    fn sat_add(&self, other: &Self) -> Self {
        self.saturating_add(other)
    }
}

impl<T: AddAssign + Copy + Default + SaturationFloatAdd> MeasurementCollection<T> {
    pub fn accumulate(&self) -> BTreeMap<String, MeasureValues<T>> {
        let mut result = BTreeMap::new();

        for entry in self.data.iter() {
            for (k, v) in entry.iter() {
                let data: &mut MeasureValues<T> = result.entry(k.clone()).or_default();

                if data.number_of_measurements == 0 {
                    data.clone_from(v);
                } else {
                    let old_n = data.number_of_measurements as f64;
                    let new_n = v.number_of_measurements as f64;
                    let n = old_n + new_n as f64;
                    data.number_of_measurements += data.number_of_measurements;
                    let exclusive_avg = data.exclusive_cumulative_sum_of_squares * (old_n / n)
                        + v.exclusive_cumulative_sum_of_squares * (new_n / n);
                    let total_avg = data.total_cumulative_average * (old_n / n)
                        + v.total_cumulative_average * (new_n / n);

                    data.total_time = data.total_time.sat_add(&v.total_time);
                    data.exclusive_time = data.exclusive_time.sat_add(&v.exclusive_time);

                    data.exclusive_cumulative_sum_of_squares = data
                        .exclusive_cumulative_sum_of_squares
                        + data.exclusive_cumulative_average
                        + v.exclusive_cumulative_sum_of_squares
                        + v.exclusive_cumulative_average
                        - exclusive_avg;
                    data.total_cumulative_sum_of_squares = data.total_cumulative_sum_of_squares
                        + data.total_cumulative_average
                        + v.total_cumulative_sum_of_squares
                        + v.total_cumulative_average
                        - total_avg;
                    data.exclusive_cumulative_average = exclusive_avg;
                    data.total_cumulative_average = total_avg;
                }
            }
        }

        result
    }
}

impl Display for MeasurementCollection<f64>{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let mut acc =self.accumulate().into_iter().collect::<Vec<(String, MeasureValues<f64>)>>();
        acc.sort_by(|a, b| {
            if a.1.exclusive_time < b.1.exclusive_time {
                core::cmp::Ordering::Less
            } else if a.1.exclusive_time > b.1.exclusive_time {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        });
        writeln!(f,
            "{:<40} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10}",
            "Name",
            "Ex. AVG",
            "Ex. VAR",
            "Total AVG",
            "Total VAR",
            "n",
            "Total excl",
            "Total time"
        )?;
        for (k, v) in acc
            .iter()
            .rev()
        {
            let (avg, avg_unit) = format_duration(v.exclusive_cumulative_average);
            let (var, var_unit) = format_duration(v.variance());
            let (total_avg, total_avg_unit) =
                format_duration(v.total_cumulative_average);
            let (total_var, total_var_unit) =
                format_duration(v.total_cumulative_sum_of_squares);
            let (total, total_unit) = format_duration(v.total_time);
            let (total_exclusive, total_exclusive_unit) =
                format_duration(v.exclusive_time);
            writeln!(f,
                "{:<40} | {:<7.3} {} | {:<7.3} {} | {:<7.3} {} | {:<7.3} {} | {:<10.1e} | {:<7.3} {} | {:<7.3} {}",
                k, avg, avg_unit, var, var_unit, total_avg, total_avg_unit, total_var, total_var_unit, v.number_of_measurements as f64, total_exclusive, total_exclusive_unit, total, total_unit
            )?;
        }
        write!(f, "")
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MeasurementManager {
    pub data: BTreeMap<&'static str, MeasureValues<u64>>,
    #[serde(skip)]
    pub exclusive_time_keeping: BTreeMap<u64, (&'static str, Duration)>,
}

impl MeasurementManager {
    pub fn new() -> Self {
        Self {
            data: BTreeMap::new(),
            exclusive_time_keeping: BTreeMap::new(),
        }
    }

    pub fn register_data_point(
        &mut self,
        name: &'static str,
        total_duration: Duration,
        exclusive_duration: Duration,
    ) {
        let x_tot = instance().duration_to_seconds(total_duration);
        let x_exclusive = instance().duration_to_seconds(exclusive_duration);

        let data = self.data.entry(name).or_default();
        if data.number_of_measurements == 0 {
            data.exclusive_cumulative_average = x_exclusive;
            data.exclusive_cumulative_sum_of_squares = 0.0;
            data.exclusive_time = exclusive_duration.0;

            data.number_of_measurements = 1;

            data.total_cumulative_average = x_tot;
            data.total_cumulative_sum_of_squares = 0.0;
            data.total_time = total_duration.0;
        } else if data.number_of_measurements == u64::MAX {
            return;
        } else {
            // see Welford's online algorithm

            let delta_exclusive = x_exclusive - data.exclusive_cumulative_average;
            let delta_tot = x_tot - data.total_cumulative_average;

            data.number_of_measurements += 1;

            data.exclusive_cumulative_average += delta_exclusive / (data.number_of_measurements as f64);
            data.total_cumulative_average += delta_tot / (data.number_of_measurements as f64);

            let delta_exclusive2 = x_exclusive - data.exclusive_cumulative_average;
            let delta_tot2 = x_tot - data.total_cumulative_average;

            data.exclusive_cumulative_sum_of_squares += delta_exclusive * delta_exclusive2;
            data.total_cumulative_sum_of_squares += delta_tot * delta_tot2;

            data.total_time = data.total_time.saturating_add(total_duration.0);
            data.exclusive_time = data.exclusive_time.saturating_add(exclusive_duration.0);
        }

        self.exclusive_time_keeping
            .iter_mut()
            .filter(|(_, v)| v.0 != name)
            .for_each(|(_, v)| v.1 += exclusive_duration);
    }

    pub fn reset(&mut self) {
        self.data.clear();
    }

    pub fn register_exclusive_measurement(&mut self, owner: &'static str) -> ExclusiveMeasurementGuard {
        let guard = ExclusiveMeasurementGuard::begin();
        self.exclusive_time_keeping.insert(guard.index, (owner, Duration(0)));
        guard
    }

    fn drop_exclusive_measurement(&mut self, index: u64) -> Duration {
        if let Some(duration) = self.exclusive_time_keeping.remove(&index) {
            duration.1
        } else {
            Duration(0)
        }
    }
}

impl Display for MeasurementManager {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "{:<20} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10} | {:<10}",
            "Name", "Ex. AVG", "Ex. VAR", "Total AVG", "Total VAR", "n", "Total time"
        )?;
        for (k, v) in mm_instance().borrow().data.iter() {
            let (avg, avg_unit) = format_duration(v.exclusive_cumulative_average);
            let (var, var_unit) = format_duration(v.variance());
            let (total_avg, total_avg_unit) = format_duration(v.total_cumulative_average);
            let (total_var, total_var_unit) = format_duration(v.total_cumulative_sum_of_squares);
            let (total, total_unit) =
                format_duration(instance().duration_to_seconds(v.total_time as f64));
            writeln!(
                f,
                "{:<20} | {:<7.3} {} | {:<7.3} {} | {:<7.3} {} | {:<7.3} {} | {:<10.1e} | {:<10.3} {}",
                k, avg, avg_unit, var, var_unit, total_avg, total_avg_unit, total_var, total_var_unit, v.number_of_measurements as f64, total, total_unit
            )?;
        }

        Ok(())
    }
}

pub fn format_duration(duration: f64) -> (f64, &'static str) {
    if duration >= 60.0 * 60.0 * 24.0 * 7.0 {
        (duration / (60.0 * 60.0 * 24.0 * 7.0), " w")
    } else if duration >= 60.0 * 60.0 * 24.0 {
        (duration / (60.0 * 60.0 * 24.0), " d")
    } else if duration >= 60.0 * 60.0 {
        (duration / (60.0 * 60.0), " h")
    } else if duration >= 60.0 {
        (duration / 60.0, " m")
    } else if duration >= 1.0 {
        (duration, " s")
    } else if duration >= 1e-3 {
        (duration * 1e3, "ms")
    } else if duration >= 1e-6 {
        (duration * 1e6, "us")
    } else {
        (duration * 1e9, "ns")
    }
}

#[repr(transparent)]
pub struct ExclusiveMeasurementGuard {
    index: u64,
}

static TIME_MEASUREMENT_INDEX: AtomicU64 = AtomicU64::new(0);

impl ExclusiveMeasurementGuard {
    pub fn begin() -> Self {
        let index = TIME_MEASUREMENT_INDEX.fetch_add(1, Ordering::SeqCst);
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
