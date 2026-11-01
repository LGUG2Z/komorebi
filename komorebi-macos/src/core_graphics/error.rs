use objc2_core_graphics::CGError;

#[derive(thiserror::Error, Debug)]
pub enum CoreGraphicsError {
    #[error("CGError::Success")]
    Success,
    #[error("CGError::Failure")]
    Failure,
    #[error("CGError::IllegalArgument")]
    IllegalArgument,
    #[error("CGError::InvalidConnection")]
    InvalidConnection,
    #[error("CGError::InvalidContext")]
    InvalidContext,
    #[error("CGError::CannotComplete")]
    CannotComplete,
    #[error("CGError::NotImplemented")]
    NotImplemented,
    #[error("CGError::RangeCheck")]
    RangeCheck,
    #[error("CGError::TypeCheck")]
    TypeCheck,
    #[error("CGError::InvalidOperation")]
    InvalidOperation,
    #[error("CGError::NoneAvailable")]
    NoneAvailable,
    #[error("CGError::Unknown {0}")]
    Unknown(i32),
}

impl From<CGError> for CoreGraphicsError {
    fn from(value: CGError) -> Self {
        match value {
            CGError::Success => Self::Success,
            CGError::Failure => Self::Failure,
            CGError::IllegalArgument => Self::IllegalArgument,
            CGError::InvalidConnection => Self::InvalidConnection,
            CGError::InvalidContext => Self::InvalidContext,
            CGError::CannotComplete => Self::CannotComplete,
            CGError::NotImplemented => Self::NotImplemented,
            CGError::RangeCheck => Self::RangeCheck,
            CGError::TypeCheck => Self::TypeCheck,
            CGError::InvalidOperation => Self::InvalidOperation,
            CGError::NoneAvailable => Self::NoneAvailable,
            error => Self::Unknown(error.0),
        }
    }
}
