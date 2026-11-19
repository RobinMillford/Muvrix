//! Safe, idiomatic Rust representations of mpv events.

use crate::error::MpvError;
use crate::property::PropertyFormat;
use std::ffi::CStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndFileReason {
    Eof,
    Stop,
    Quit,
    Error,
    Redirect,
    Unknown(i32),
}

impl EndFileReason {
    pub fn from_raw(raw: libmpv_sys::mpv_end_file_reason) -> Self {
        match raw {
            libmpv_sys::mpv_end_file_reason::MPV_END_FILE_REASON_EOF => Self::Eof,
            libmpv_sys::mpv_end_file_reason::MPV_END_FILE_REASON_STOP => Self::Stop,
            libmpv_sys::mpv_end_file_reason::MPV_END_FILE_REASON_QUIT => Self::Quit,
            libmpv_sys::mpv_end_file_reason::MPV_END_FILE_REASON_ERROR => Self::Error,
            libmpv_sys::mpv_end_file_reason::MPV_END_FILE_REASON_REDIRECT => Self::Redirect,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    None,
    Shutdown,
    LogMessage {
        prefix: String,
        level: String,
        text: String,
        log_level: i32,
    },
    GetPropertyReply {
        reply_userdata: u64,
        error: Result<(), MpvError>,
    },
    SetPropertyReply {
        reply_userdata: u64,
        error: Result<(), MpvError>,
    },
    CommandReply {
        reply_userdata: u64,
        error: Result<(), MpvError>,
    },
    StartFile {
        playlist_entry_id: i64,
    },
    EndFile {
        reason: EndFileReason,
        error: Option<MpvError>,
        playlist_entry_id: i64,
    },
    FileLoaded,
    Idle,
    Tick,
    ClientMessage(Vec<String>),
    VideoReconfig,
    AudioReconfig,
    Seek,
    PlaybackRestart,
    PropertyChange {
        reply_userdata: u64,
        name: String,
        format: PropertyFormat,
        /// Value carried inline by the observation (when registered with a
        /// typed format instead of `PropertyFormat::None`). Lets the hot path
        /// consume property changes without a blocking `mpv_get_property`
        /// round trip, which under decode pressure stalls the caller for
        /// tens to hundreds of milliseconds.
        value: Option<PropertyChangeValue>,
    },
    QueueOverflow,
    Hook {
        name: String,
        id: u64,
    },
    Unknown(i32),
}

/// Scalar property value delivered inline by a typed property observation.
///
/// `Node` payloads (track lists, playlists, device lists) are not decoded here;
/// those observations are registered with `PropertyFormat::None` and re-read on
/// demand, as before.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyChangeValue {
    Flag(bool),
    Int64(i64),
    Double(f64),
}

impl Event {
    /// Converts a raw `libmpv_sys::mpv_event` pointer to a safe, owned `Event`.
    ///
    /// # Safety
    /// `raw` must be a valid pointer returned by `mpv_wait_event`.
    pub unsafe fn from_raw(raw: *mut libmpv_sys::mpv_event) -> Self {
        if raw.is_null() {
            return Self::None;
        }

        let event = &*raw;
        let id = event.event_id as i32;

        match id {
            0 => Self::None,
            1 => Self::Shutdown,
            2 => {
                if !event.data.is_null() {
                    let log = &*(event.data as *const libmpv_sys::mpv_event_log_message);
                    let prefix = if log.prefix.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(log.prefix).to_string_lossy().into_owned()
                    };
                    let level = if log.level.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(log.level).to_string_lossy().into_owned()
                    };
                    let text = if log.text.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(log.text).to_string_lossy().into_owned()
                    };
                    Self::LogMessage {
                        prefix,
                        level,
                        text,
                        log_level: log.log_level as i32,
                    }
                } else {
                    Self::LogMessage {
                        prefix: String::new(),
                        level: String::new(),
                        text: String::new(),
                        log_level: 0,
                    }
                }
            }
            3 => Self::GetPropertyReply {
                reply_userdata: event.reply_userdata,
                error: MpvError::from_raw(event.error),
            },
            4 => Self::SetPropertyReply {
                reply_userdata: event.reply_userdata,
                error: MpvError::from_raw(event.error),
            },
            5 => Self::CommandReply {
                reply_userdata: event.reply_userdata,
                error: MpvError::from_raw(event.error),
            },
            6 => {
                let playlist_entry_id = if !event.data.is_null() {
                    let start = &*(event.data as *const libmpv_sys::mpv_event_start_file);
                    start.playlist_entry_id
                } else {
                    0
                };
                Self::StartFile { playlist_entry_id }
            }
            7 => {
                if !event.data.is_null() {
                    let end = &*(event.data as *const libmpv_sys::mpv_event_end_file);
                    let reason = EndFileReason::from_raw(end.reason);
                    let error = if end.error != 0 {
                        MpvError::from_raw(end.error).err()
                    } else {
                        None
                    };
                    Self::EndFile {
                        reason,
                        error,
                        playlist_entry_id: end.playlist_entry_id,
                    }
                } else {
                    Self::EndFile {
                        reason: EndFileReason::Unknown(0),
                        error: None,
                        playlist_entry_id: 0,
                    }
                }
            }
            8 => Self::FileLoaded,
            11 => Self::Idle,
            14 => Self::Tick,
            16 => {
                let mut args = Vec::new();
                if !event.data.is_null() {
                    let msg = &*(event.data as *const libmpv_sys::mpv_event_client_message);
                    for i in 0..msg.num_args {
                        let arg_ptr = *msg.args.add(i as usize);
                        if !arg_ptr.is_null() {
                            args.push(CStr::from_ptr(arg_ptr).to_string_lossy().into_owned());
                        }
                    }
                }
                Self::ClientMessage(args)
            }
            17 => Self::VideoReconfig,
            18 => Self::AudioReconfig,
            20 => Self::Seek,
            21 => Self::PlaybackRestart,
            22 => {
                let (name, format, value) = if !event.data.is_null() {
                    let prop = &*(event.data as *const libmpv_sys::mpv_event_property);
                    let name = if prop.name.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(prop.name).to_string_lossy().into_owned()
                    };
                    let format = PropertyFormat::from_raw(prop.format);
                    let value = decode_property_value(format, prop.data);
                    (name, format, value)
                } else {
                    (String::new(), PropertyFormat::None, None)
                };
                Self::PropertyChange {
                    reply_userdata: event.reply_userdata,
                    name,
                    format,
                    value,
                }
            }
            24 => Self::QueueOverflow,
            25 => {
                let name = if !event.data.is_null() {
                    let hook = &*(event.data as *const libmpv_sys::mpv_event_hook);
                    if hook.name.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(hook.name).to_string_lossy().into_owned()
                    }
                } else {
                    String::new()
                };
                let id = if !event.data.is_null() {
                    let hook = &*(event.data as *const libmpv_sys::mpv_event_hook);
                    hook.id
                } else {
                    0
                };
                Self::Hook { name, id }
            }
            other => Self::Unknown(other),
        }
    }
}

/// Decodes the payload of a typed property-change observation.
///
/// `data` may be null (mpv coalesced an unchanged value) or a `Node` that this
/// client does not decode. Both cases yield `None`; callers fall back to
/// re-reading the property, exactly as they did before typed observations
/// existed.
fn decode_property_value(
    format: PropertyFormat,
    data: *mut std::os::raw::c_void,
) -> Option<PropertyChangeValue> {
    if data.is_null() {
        return None;
    }
    match format {
        PropertyFormat::Flag => Some(PropertyChangeValue::Flag(unsafe {
            *(data as *const std::os::raw::c_int) != 0
        })),
        PropertyFormat::Int64 => Some(PropertyChangeValue::Int64(unsafe { *(data as *const i64) })),
        PropertyFormat::Double => {
            let v = unsafe { *(data as *const f64) };
            if v.is_finite() {
                Some(PropertyChangeValue::Double(v))
            } else {
                None
            }
        }
        // String-carrying events are deliberately not decoded: ownership of
        // the `char*` inside `mpv_event_property` is unspecified across
        // versions, and the string-typed properties (hwdec-current,
        // current-ao) change rarely enough that the fallback re-read is fine.
        PropertyFormat::String
        | PropertyFormat::OsdString
        | PropertyFormat::None
        | PropertyFormat::Node => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop_event(format: PropertyFormat) -> Event {
        Event::PropertyChange {
            reply_userdata: 1,
            name: "time-pos".to_string(),
            format,
            value: None,
        }
    }

    #[test]
    fn untyped_observation_carries_no_inline_value() {
        // A `PropertyFormat::None` observation has no inline payload; the
        // client must fall back to a synchronous read, which is why the hot
        // properties are observed with typed formats instead.
        let ev = prop_event(PropertyFormat::None);
        match ev {
            Event::PropertyChange { value, .. } => assert!(value.is_none()),
            _ => panic!("wrong event variant"),
        }
    }

    #[test]
    fn double_payload_decodes_from_raw_pointer() {
        let val: f64 = 12.5;
        let ptr = &val as *const f64 as *mut std::os::raw::c_void;
        assert_eq!(
            decode_property_value(PropertyFormat::Double, ptr),
            Some(PropertyChangeValue::Double(12.5))
        );
    }

    #[test]
    fn flag_payload_decodes_from_raw_pointer() {
        let val: std::os::raw::c_int = 1;
        let ptr = &val as *const _ as *mut std::os::raw::c_void;
        assert_eq!(
            decode_property_value(PropertyFormat::Flag, ptr),
            Some(PropertyChangeValue::Flag(true))
        );
        let zero: std::os::raw::c_int = 0;
        let ptr = &zero as *const _ as *mut std::os::raw::c_void;
        assert_eq!(
            decode_property_value(PropertyFormat::Flag, ptr),
            Some(PropertyChangeValue::Flag(false))
        );
    }

    #[test]
    fn int64_payload_decodes_from_raw_pointer() {
        let val: i64 = 42;
        let ptr = &val as *const i64 as *mut std::os::raw::c_void;
        assert_eq!(
            decode_property_value(PropertyFormat::Int64, ptr),
            Some(PropertyChangeValue::Int64(42))
        );
    }

    #[test]
    fn node_and_none_formats_decode_to_none() {
        let val: i64 = 42;
        let ptr = &val as *const i64 as *mut std::os::raw::c_void;
        assert_eq!(decode_property_value(PropertyFormat::Node, ptr), None);
        assert_eq!(decode_property_value(PropertyFormat::None, ptr), None);
    }

    #[test]
    fn null_payload_decodes_to_none() {
        assert_eq!(
            decode_property_value(PropertyFormat::Double, std::ptr::null_mut()),
            None
        );
    }

    #[test]
    fn non_finite_double_is_not_reported() {
        let val: f64 = f64::NAN;
        let ptr = &val as *const f64 as *mut std::os::raw::c_void;
        assert_eq!(decode_property_value(PropertyFormat::Double, ptr), None);
    }
}
