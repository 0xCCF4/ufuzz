use thiserror_no_std::Error;

#[derive(Error, Debug)]
pub enum HypervisorError {
    #[error("Guest VM ran out of LDT entries")]
    NestedPagingStructuresExhausted,
}

pub type Result<T> = core::result::Result<T, HypervisorError>;
