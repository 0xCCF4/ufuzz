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

pub struct PerfMonitor {
    pub measurement_data: MeasurementCollection<u64>,
    pub filename: CString16,
    pub last_save: Instant,
}

impl PerfMonitor {
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

    pub fn update_values(&mut self, data: &MeasurementData<u64>) {
        let last = self.measurement_data.data.last_mut().unwrap();
        last.clone_from(data);
    }

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
