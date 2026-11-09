use std::ffi::CStr;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum MpvError {
    #[error("Event ringbuffer is full")]
    EventQueueFull,

    #[error("Memory allocation failed")]
    NoMem,

    #[error("Core uninitialized")]
    Uninitialized,

    #[error("Invalid parameter")]
    InvalidParameter,

    #[error("Option not found")]
    OptionNotFound,

    #[error("Unsupported option format")]
    OptionFormat,

    #[error("Error setting option")]
    OptionError,

    #[error("Property not found")]
    PropertyNotFound,

    #[error("Unsupported property format")]
    PropertyFormat,

    #[error("Property currently unavailable")]
    PropertyUnavailable,

    #[error("Error accessing property")]
    PropertyError,

    #[error("Command execution error")]
    Command,

    #[error("Loading file failed")]
    LoadingFailed,

    #[error("Audio output initialization failed")]
    AoInitFailed,

    #[error("Video output initialization failed")]
    VoInitFailed,

    #[error("Nothing to play")]
    NothingToPlay,

    #[error("Unknown or invalid media format")]
    UnknownFormat,

    #[error("Unsupported feature or codec")]
    Unsupported,

    #[error("Function or backend not implemented")]
    NotImplemented,

    #[error("Generic unclassified error")]
    Generic,

    #[error("String contains interior NUL byte: {0}")]
    InvalidString(String),

    #[error("Mpv handle is null or already terminated")]
    NullHandle,

    #[error("API version incompatibility: required {required}, runtime found {found}")]
    IncompatibleApiVersion { required: String, found: String },

    #[error("Security violation: {0}")]
    SecurityViolation(String),

    #[error("Mpv error ({code}): {description}")]
    Raw { code: i32, description: String },
}

impl MpvError {
    pub fn from_raw(code: i32) -> Result<(), Self> {
        if code >= 0 {
            return Ok(());
        }

        let desc = unsafe {
            let ptr = libmpv_sys::mpv_error_string(code);
            if ptr.is_null() {
                "Unknown error".to_string()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        };

        match code {
            c if c == libmpv_sys::mpv_error::MPV_ERROR_EVENT_QUEUE_FULL as i32 => {
                Err(Self::EventQueueFull)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_NOMEM as i32 => Err(Self::NoMem),
            c if c == libmpv_sys::mpv_error::MPV_ERROR_UNINITIALIZED as i32 => {
                Err(Self::Uninitialized)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_INVALID_PARAMETER as i32 => {
                Err(Self::InvalidParameter)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_OPTION_NOT_FOUND as i32 => {
                Err(Self::OptionNotFound)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_OPTION_FORMAT as i32 => {
                Err(Self::OptionFormat)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_OPTION_ERROR as i32 => {
                Err(Self::OptionError)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_PROPERTY_NOT_FOUND as i32 => {
                Err(Self::PropertyNotFound)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_PROPERTY_FORMAT as i32 => {
                Err(Self::PropertyFormat)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_PROPERTY_UNAVAILABLE as i32 => {
                Err(Self::PropertyUnavailable)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_PROPERTY_ERROR as i32 => {
                Err(Self::PropertyError)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_COMMAND as i32 => Err(Self::Command),
            c if c == libmpv_sys::mpv_error::MPV_ERROR_LOADING_FAILED as i32 => {
                Err(Self::LoadingFailed)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_AO_INIT_FAILED as i32 => {
                Err(Self::AoInitFailed)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_VO_INIT_FAILED as i32 => {
                Err(Self::VoInitFailed)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_NOTHING_TO_PLAY as i32 => {
                Err(Self::NothingToPlay)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_UNKNOWN_FORMAT as i32 => {
                Err(Self::UnknownFormat)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_UNSUPPORTED as i32 => Err(Self::Unsupported),
            c if c == libmpv_sys::mpv_error::MPV_ERROR_NOT_IMPLEMENTED as i32 => {
                Err(Self::NotImplemented)
            }
            c if c == libmpv_sys::mpv_error::MPV_ERROR_GENERIC as i32 => Err(Self::Generic),
            _ => Err(Self::Raw {
                code,
                description: desc,
            }),
        }
    }
}
