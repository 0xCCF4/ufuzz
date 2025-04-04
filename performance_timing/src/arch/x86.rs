use crate::measurements::mm_initialize;
use crate::{instance, Availability, Instant, INITIALIZED, INSTANCE};
use core::error::Error;
use core::fmt::{Display, Formatter};
use core::sync::atomic::Ordering;
use x86::cpuid::cpuid;
use x86::fence::{lfence, sfence};
use x86::time::rdtsc;

enum Vendor {
    Intel,
    AMD,
    Unknown,
}

fn processor_vendor() -> Vendor {
    match x86::cpuid::CpuId::new()
        .get_vendor_info()
        .as_ref()
        .map(|x| x.as_str())
    {
        Some("GenuineIntel") => Vendor::Intel,
        Some("AuthenticAMD") => Vendor::AMD,
        _ => Vendor::Unknown,
    }
}

// only single thread safe
pub struct TimeKeeper {
    p0_frequency_ghz: f64,
}

#[derive(Copy, Clone, PartialEq, Hash, Debug, Eq)]
pub enum CreationError {
    NotAvailable,
    TSCFrequency,
}

impl Display for CreationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            CreationError::NotAvailable => write!(f, "TimeKeeper is not available on this CPU"),
            CreationError::TSCFrequency => write!(f, "Unable to get TSC frequency"),
        }
    }
}

impl Error for CreationError {}

impl TimeKeeper {
    pub fn availability() -> Availability {
        match processor_vendor() {
            Vendor::Intel | Vendor::AMD => {
                if (cpuid!(0x80000007).edx & (1 << 8)) > 0 {
                    Availability::Full
                } else {
                    Availability::Partial
                }
            }
            Vendor::Unknown => Availability::None,
        }
    }
    pub fn new(p0_frequency_ghz: f64) -> Result<Self, CreationError> {
        let availability = Self::availability();

        if availability == Availability::None {
            return Err(CreationError::NotAvailable);
        }

        Ok(Self { p0_frequency_ghz })
    }

    pub fn now(&self) -> Instant {
        lfence();
        sfence();
        let time = unsafe { rdtsc() };
        lfence();

        Instant::new(time)
    }

    pub fn duration_to_seconds<T: Into<f64>>(&self, duration: T) -> f64 {
        duration.into() / self.p0_frequency_ghz as f64
    }
}

pub fn initialize(system_p0_frequency_ghz: f64) -> Result<&'static TimeKeeper, impl Error> {
    mm_initialize();
    if INITIALIZED.load(Ordering::Relaxed) {
        return Ok(instance());
    }
    unsafe {
        INSTANCE = Some(TimeKeeper::new(system_p0_frequency_ghz)?);
    }
    INITIALIZED.store(true, Ordering::Relaxed);
    Ok::<&'static TimeKeeper, CreationError>(instance())
}

pub type TimeStamp = u64;

#[cfg(test)]
mod test {
    use crate::measurements::mm_instance;
    use crate::{initialize, TimeKeeper, TimeMeasurement};

    #[test]
    pub fn test() {
        let tk = TimeKeeper::new(2_699_000_000f64).expect("Unable to create time keeper");
        let now = tk.now();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let then = tk.now();
        println!("Duration: {}", tk.duration_to_seconds(then - now));

        initialize(2_699_000_000f64);

        for i in 0..100 {
            let m = TimeMeasurement::begin_exclusive("hello");
            for i in 0..100 {
                let m = TimeMeasurement::begin("test");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        for i in 0..100 {
            let m = TimeMeasurement::begin("hello2");
            for i in 0..100 {
                let m = TimeMeasurement::begin("test2");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        println!("{}", mm_instance().borrow());
    }
}
