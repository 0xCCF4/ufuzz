//! Performance Monitor Module
//!
//! This module provides functionality for monitoring and recording performance metrics
//! during fuzzing operations.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use log::error;
use performance_timing::measurements::{MeasureValues, MeasurementCollection, MeasurementData};
#[cfg(feature = "__debug_performance_trace")]
use performance_timing::track_time;
use performance_timing::Instant;
use uefi::proto::media::file::{File, FileMode};
use uefi::CString16;
use uefi_raw::protocol::file_system::FileAttribute;
use uefi_raw::Status;

/// Performance monitoring and measurement collection
///
/// This structure manages the collection and storage of performance measurements,
/// providing functionality for updating measurements and saving them to a file.
pub struct PerfMonitor {
    /// Collection of performance measurements
    pub measurement_data: MeasurementCollection<u64>,
    /// Path to the measurement data file
    pub filename: CString16,
    /// Timestamp of the last save operation
    pub last_save: Instant,
}

impl PerfMonitor {
    /// Creates a new performance monitor instance by loading it from a file (if it exists)
    ///
    /// # Arguments
    ///
    /// * `filepath` - Path to the measurement data file
    ///
    /// # Returns
    ///
    /// * `uefi::Result<PerfMonitor>` - New monitor instance or error
    pub fn new(filepath: &str) -> uefi::Result<PerfMonitor> {
        let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
        let mut root_dir = proto.open_volume()?;
        let filename = CString16::try_from(filepath)
            .map_err(|_| uefi::Error::from(uefi::Status::UNSUPPORTED))?;
        let file = match root_dir.open(filename.as_ref(), FileMode::Read, FileAttribute::empty()) {
            Ok(file) => file,
            Err(e) => {
                return if e.status() == Status::NOT_FOUND {
                    let mut result = Self {
                        measurement_data: MeasurementCollection::default(),
                        filename,
                        last_save: performance_timing::instance().now(),
                    };
                    result
                        .measurement_data
                        .data
                        .push(MeasurementData::default());
                    Ok(result)
                } else {
                    Err(e)
                }
            }
        };
        let mut regular_file = file
            .into_regular_file()
            .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;

        let mut buffer = [0u8; 4096];
        let mut data = String::new();

        loop {
            let read = regular_file.read(&mut buffer)?;

            if read == 0 {
                break;
            }

            for i in 0..read {
                data.push(buffer[i] as char);
            }
        }

        let mut data: MeasurementCollection<u64> = serde_json::from_str(data.as_str())
            .unwrap_or_else(|e| {
                error!("Json deserialize error: {:?}", e);
                let mut result = MeasurementCollection::default();
                result.data.push(MeasurementData::default());
                result
            });

        data.data.push(MeasurementData::default());

        Ok(Self {
            measurement_data: data,
            filename,
            last_save: performance_timing::instance().now(),
        })
    }

    /// Updates measurements from the performance timing monitor
    ///
    /// This function collects current measurements from the performance timing
    /// system and updates the stored measurements.
    #[cfg_attr(
        feature = "__debug_performance_trace",
        track_time("perf::update_values")
    )]
    pub fn update_values_from_monitor(&mut self) {
        let measurements = performance_timing::measurements::mm_instance()
            .borrow()
            .data
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<BTreeMap<String, MeasureValues<u64>>>();

        self.update_values(&measurements);
    }

    /// Updates stored measurements with new data
    ///
    /// # Arguments
    ///
    /// * `data` - New measurement data to store
    pub fn update_values(&mut self, data: &MeasurementData<u64>) {
        let last = self.measurement_data.data.last_mut().unwrap();
        last.clone_from(data);
    }

    /// Attempts to save measurements to file if enough time has passed
    ///
    /// Measurements are saved if more than 30 seconds have passed since the last save.
    ///
    /// # Returns
    ///
    /// * `uefi::Result<()>` - Success or error
    pub fn try_save_file(&mut self) -> uefi::Result<()> {
        let now = performance_timing::instance().now();
        let duration = now - self.last_save;
        let duration_s = performance_timing::instance().duration_to_seconds(duration);
        if duration_s > 30.0 {
            self.last_save = now;
            self.save_file()
        } else {
            Ok(())
        }
    }

    /// Updates measurements and saves to file if enough time has passed
    ///
    /// This function combines updating measurements from the monitor and saving
    /// to file if more than 30 seconds have passed since the last save.
    ///
    /// # Returns
    ///
    /// * `uefi::Result<()>` - Success or error
    pub fn try_update_save_file(&mut self) -> uefi::Result<()> {
        let now = performance_timing::instance().now();
        let duration = now - self.last_save;
        let duration_s = performance_timing::instance().duration_to_seconds(duration);
        if duration_s > 30.0 {
            self.last_save = now;
            self.update_values_from_monitor();
            self.save_file()
        } else {
            Ok(())
        }
    }

    /// Saves the current measurements to file
    ///
    /// # Returns
    ///
    /// * `uefi::Result<()>` - Success or error
    #[cfg_attr(feature = "__debug_performance_trace", track_time("perf::save_file"))]
    pub fn save_file(&self) -> uefi::Result<()> {
        let mut proto = uefi::boot::get_image_file_system(uefi::boot::image_handle())?;
        let mut root_dir = proto.open_volume()?;
        let file = root_dir.open(
            self.filename.as_ref(),
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )?;
        let mut regular_file = file
            .into_regular_file()
            .ok_or_else(|| uefi::Error::from(uefi::Status::UNSUPPORTED))?;
        let data = serde_json::to_string(&self.measurement_data).map_err(|e| {
            error!("Failed to serialize measurement data: {:?}", e);
            uefi::Status::ABORTED
        })?;

        for data_chunked in data.as_bytes().chunks(4096) {
            regular_file
                .write(data_chunked)
                .map_err(|_| uefi::Error::from(uefi::Status::WARN_WRITE_FAILURE))?;
        }
        regular_file.flush()?;

        root_dir.flush()?;
        root_dir.close();

        Ok(())
    }
}
