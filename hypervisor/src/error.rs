use thiserror_no_std::Error;

#[derive(Error, Debug)]
pub enum HypervisorError {
    #[error("Guest VM ran out of LDT entries")]
    NestedPagingStructuresExhausted,

    #[error("Failed to enable virtualization on the CPU")]
    FailedToInitializeHost(&'static str),
}

pub type Result<T> = core::result::Result<T, HypervisorError>;
