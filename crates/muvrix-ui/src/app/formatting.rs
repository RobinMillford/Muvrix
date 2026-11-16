use muvrix_player::TrackInfo;

/// Formats seconds into standard HH:MM:SS or MM:SS presentation.
pub fn format_timestamp(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00".to_string();
    }
    let total_secs = seconds.floor() as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

/// Formats seconds into millisecond-accurate HH:MM:SS.mmm or MM:SS.mmm presentation.
pub fn format_timestamp_ms(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00.000".to_string();
    }
    let total_secs = seconds.floor() as u64;
    let millis = ((seconds.fract() * 1000.0).round() as u64).min(999);
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}.{millis:03}")
    } else {
        format!("{mins:02}:{secs:02}.{millis:03}")
    }
}

/// Calculates normalized progress fraction clamped to [0.0, 1.0].
pub fn calculate_progress(time_pos: f64, duration: f64) -> f32 {
    if duration <= 0.0 || time_pos <= 0.0 || time_pos.is_nan() || duration.is_nan() {
        0.0
    } else {
        ((time_pos / duration) as f32).clamp(0.0, 1.0)
    }
}

/// Formats playback speed factor into a concise display string.
pub fn format_speed(speed: f64) -> String {
    if (speed - 1.0).abs() < 0.001 {
        "1.0x (Normal)".to_string()
    } else {
        format!("{:.2}x", speed)
    }
}

/// Formats volume and mute state for user display.
pub fn format_volume(volume: f64, is_muted: bool) -> String {
    if is_muted {
        "Muted (0%)".to_string()
    } else {
        format!("{:.0}%", volume.clamp(0.0, 100.0))
    }
}

/// Formats a raw codec name into standard user-facing presentation.
pub fn format_codec_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "h264" | "avc" | "avc1" => "H.264".to_string(),
        "hevc" | "h265" | "hev1" => "HEVC".to_string(),
        "vp9" => "VP9".to_string(),
        "av1" | "av01" => "AV1".to_string(),
        "vp8" => "VP8".to_string(),
        "aac" | "mp4a" => "AAC".to_string(),
        "opus" => "Opus".to_string(),
        "flac" => "FLAC".to_string(),
        "mp3" => "MP3".to_string(),
        "ac3" => "AC-3".to_string(),
        "eac3" => "E-AC-3".to_string(),
        "truehd" => "TrueHD".to_string(),
        "dts" => "DTS".to_string(),
        "vorbis" => "Vorbis".to_string(),
        other => other.to_string(),
    }
}

/// Formats a media track for user menus with descriptive labels.
pub fn format_track_label(track: &TrackInfo) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &track.title {
        let t = title.trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    if let Some(lang) = &track.lang {
        let l = lang.trim();
        if !l.is_empty() {
            let lower = l.to_lowercase();
            let lang_clean = match lower.as_str() {
                "eng" | "en" => "English",
                "jpn" | "ja" => "Japanese",
                "spa" | "es" => "Spanish",
                "fra" | "fr" => "French",
                "deu" | "ger" | "de" => "German",
                "ita" | "it" => "Italian",
                "por" | "pt" => "Portuguese",
                "rus" | "ru" => "Russian",
                "zho" | "chi" | "zh" => "Chinese",
                "kor" | "ko" => "Korean",
                _ => l,
            };
            if parts.is_empty() || !parts[0].to_lowercase().contains(&lower) {
                parts.push(lang_clean.to_string());
            }
        }
    }
    if let Some(codec) = &track.codec {
        let c = codec.trim();
        if !c.is_empty() {
            parts.push(format!("[{}]", format_codec_name(c)));
        }
    }

    if parts.is_empty() {
        format!("Track {}", track.id)
    } else {
        format!("Track {}: {}", track.id, parts.join(" · "))
    }
}
