use objc2_application_services::AXError;

#[derive(thiserror::Error, Debug)]
pub enum AccessibilityError {
    #[error(transparent)]
    Api(#[from] AccessibilityApiError),
    #[error(transparent)]
    Custom(#[from] AccessibilityCustomError),
}

#[derive(thiserror::Error, Debug)]
pub enum AccessibilityApiError {
    #[error("AXError::Success")]
    Success,
    #[error("AXError::Failure")]
    Failure,
    #[error("AXError::IllegalArgument")]
    IllegalArgument,
    #[error("AXError::InvalidUIElement")]
    InvalidUIElement,
    #[error("AXError::InvalidUIElementObserver")]
    InvalidUIElementObserver,
    #[error("AXError::CannotComplete")]
    CannotComplete,
    #[error("AXError::AttributeUnsupported")]
    AttributeUnsupported,
    #[error("AXError::ActionUnsupported")]
    ActionUnsupported,
    #[error("AXError::NotificationUnsupported")]
    NotificationUnsupported,
    #[error("AXError::NotImplemented")]
    NotImplemented,
    #[error("AXError::NotificationAlreadyRegistered")]
    NotificationAlreadyRegistered,
    #[error("AXError::NotificationNotRegistered")]
    NotificationNotRegistered,
    #[error("AXError::APIDisabled")]
    APIDisabled,
    #[error("AXError::NoValue")]
    NoValue,
    #[error("AXError::ParameterizedAttributeUnsupported")]
    ParameterizedAttributeUnsupported,
    #[error("AXError::NotEnoughPrecision")]
    NotEnoughPrecision,
    #[error("AXError::Unknown {0}")]
    Unknown(i32),
}

#[derive(thiserror::Error, Debug)]
pub enum AccessibilityCustomError {
    #[error("AXValue::create returned None")]
    AxValueCreate,
    #[error("NSRunningApplication could not be created for process {0}")]
    NSRunningApplication(i32),
}

impl From<AXError> for AccessibilityApiError {
    fn from(value: AXError) -> Self {
        match value {
            AXError::Success => Self::Success,
            AXError::Failure => Self::Failure,
            AXError::IllegalArgument => Self::IllegalArgument,
            AXError::InvalidUIElement => Self::InvalidUIElement,
            AXError::InvalidUIElementObserver => Self::InvalidUIElementObserver,
            AXError::CannotComplete => Self::CannotComplete,
            AXError::AttributeUnsupported => Self::AttributeUnsupported,
            AXError::ActionUnsupported => Self::ActionUnsupported,
            AXError::NotificationUnsupported => Self::NotificationUnsupported,
            AXError::NotImplemented => Self::NotImplemented,
            AXError::NotificationAlreadyRegistered => Self::NotificationAlreadyRegistered,
            AXError::NotificationNotRegistered => Self::NotificationNotRegistered,
            AXError::APIDisabled => Self::APIDisabled,
            AXError::NoValue => Self::NoValue,
            AXError::ParameterizedAttributeUnsupported => Self::ParameterizedAttributeUnsupported,
            AXError::NotEnoughPrecision => Self::NotEnoughPrecision,
            error => Self::Unknown(error.0),
        }
    }
}

impl From<AXError> for AccessibilityError {
    fn from(value: AXError) -> Self {
        Self::Api(AccessibilityApiError::from(value))
    }
}
