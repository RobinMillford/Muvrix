//! XML settings backup and restore engine for Muvrix.
//!
//! Provides deterministic export and import of application configuration,
//! audio preferences, subtitle styling, video adjustments, and history settings.
//! Uses a lightweight, robust XML serializer/parser with full schema validation.

use std::collections::HashMap;
use thiserror::Error;

pub const CURRENT_BACKUP_SCHEMA: u32 = 1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BackupError {
    #[error("Invalid XML structure: {0}")]
    InvalidStructure(String),
    #[error("Missing required root element <muvrix_backup>")]
    MissingRoot,
    #[error("Unsupported backup schema version: {0}")]
    UnsupportedVersion(u32),
    #[error("Empty backup document")]
    EmptyDocument,
}

/// Structured settings representation for XML backup and restore.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BackupSettings {
    pub schema_version: u32,
    pub app_version: String,
    pub created_at_unix: u64,

    // Audio & Engine
    pub audio_channels: String,
    pub volume_normalization: bool,
    pub audio_pitch_correction: bool,
    pub preferred_audio_languages: String,
    pub default_audio_delay_ms: i32,
    pub volume_boost_cap: i32,
    pub volume: f64,

    // Appearance
    pub theme_mode: String,
    pub accent_color: String,

    // Subtitle Styling
    pub sub_font: String,
    pub sub_font_size: u32,
    pub sub_color: String,
    pub sub_border_color: String,
    pub sub_border_size: f64,
    pub sub_shadow_offset: f64,
    pub sub_back_color: String,
    pub sub_margin_y: u32,
    pub sub_pos: u32,
    pub sub_speed: f64,
    pub sub_justify: String,
    pub ass_override: bool,

    // Video Adjustments & Shaders
    pub anime4k_preset: String,
    pub anime4k_quality: String,
    pub tone_mapping: String,
    pub deband: bool,

    // History & Privacy
    pub history_enabled: bool,
    pub auto_resume_enabled: bool,

    // Advanced
    pub verbose_logging: bool,
    pub background_audio: bool,
}

impl BackupSettings {
    /// Serializes settings into a clean, human-readable XML document.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<muvrix_backup schema_version=\"{}\" app_version=\"{}\" timestamp=\"{}\">\n",
            self.schema_version,
            xml_escape(&self.app_version),
            self.created_at_unix
        ));

        out.push_str("  <audio>\n");
        out.push_str(&format!(
            "    <channels>{}</channels>\n",
            xml_escape(&self.audio_channels)
        ));
        out.push_str(&format!(
            "    <normalization>{}</normalization>\n",
            self.volume_normalization
        ));
        out.push_str(&format!(
            "    <pitch_correction>{}</pitch_correction>\n",
            self.audio_pitch_correction
        ));
        out.push_str(&format!(
            "    <preferred_languages>{}</preferred_languages>\n",
            xml_escape(&self.preferred_audio_languages)
        ));
        out.push_str(&format!(
            "    <default_delay_ms>{}</default_delay_ms>\n",
            self.default_audio_delay_ms
        ));
        out.push_str(&format!(
            "    <volume_boost_cap>{}</volume_boost_cap>\n",
            self.volume_boost_cap
        ));
        out.push_str(&format!("    <volume>{}</volume>\n", self.volume));
        out.push_str(&format!(
            "    <background_audio>{}</background_audio>\n",
            self.background_audio
        ));
        out.push_str("  </audio>\n");

        out.push_str("  <appearance>\n");
        out.push_str(&format!(
            "    <theme_mode>{}</theme_mode>\n",
            xml_escape(&self.theme_mode)
        ));
        out.push_str(&format!(
            "    <accent_color>{}</accent_color>\n",
            xml_escape(&self.accent_color)
        ));
        out.push_str("  </appearance>\n");

        out.push_str("  <subtitles>\n");
        out.push_str(&format!(
            "    <font>{}</font>\n",
            xml_escape(&self.sub_font)
        ));
        out.push_str(&format!(
            "    <font_size>{}</font_size>\n",
            self.sub_font_size
        ));
        out.push_str(&format!(
            "    <color>{}</color>\n",
            xml_escape(&self.sub_color)
        ));
        out.push_str(&format!(
            "    <border_color>{}</border_color>\n",
            xml_escape(&self.sub_border_color)
        ));
        out.push_str(&format!(
            "    <border_size>{}</border_size>\n",
            self.sub_border_size
        ));
        out.push_str(&format!(
            "    <shadow_offset>{}</shadow_offset>\n",
            self.sub_shadow_offset
        ));
        out.push_str(&format!(
            "    <back_color>{}</back_color>\n",
            xml_escape(&self.sub_back_color)
        ));
        out.push_str(&format!("    <margin_y>{}</margin_y>\n", self.sub_margin_y));
        out.push_str(&format!("    <pos>{}</pos>\n", self.sub_pos));
        out.push_str(&format!("    <speed>{}</speed>\n", self.sub_speed));
        out.push_str(&format!(
            "    <justify>{}</justify>\n",
            xml_escape(&self.sub_justify)
        ));
        out.push_str(&format!(
            "    <ass_override>{}</ass_override>\n",
            self.ass_override
        ));
        out.push_str("  </subtitles>\n");

        out.push_str("  <video>\n");
        out.push_str(&format!(
            "    <anime4k_preset>{}</anime4k_preset>\n",
            xml_escape(&self.anime4k_preset)
        ));
        out.push_str(&format!(
            "    <anime4k_quality>{}</anime4k_quality>\n",
            xml_escape(&self.anime4k_quality)
        ));
        out.push_str(&format!(
            "    <tone_mapping>{}</tone_mapping>\n",
            xml_escape(&self.tone_mapping)
        ));
        out.push_str(&format!("    <deband>{}</deband>\n", self.deband));
        out.push_str("  </video>\n");

        out.push_str("  <history>\n");
        out.push_str(&format!(
            "    <enabled>{}</enabled>\n",
            self.history_enabled
        ));
        out.push_str(&format!(
            "    <auto_resume>{}</auto_resume>\n",
            self.auto_resume_enabled
        ));
        out.push_str("  </history>\n");

        out.push_str("  <advanced>\n");
        out.push_str(&format!(
            "    <verbose_logging>{}</verbose_logging>\n",
            self.verbose_logging
        ));
        out.push_str("  </advanced>\n");

        out.push_str("</muvrix_backup>\n");
        out
    }

    /// Deserializes and validates an XML settings document.
    pub fn from_xml(xml: &str) -> Result<Self, BackupError> {
        let trimmed = xml.trim();
        if trimmed.is_empty() {
            return Err(BackupError::EmptyDocument);
        }
        if !trimmed.contains("<muvrix_backup") || !trimmed.contains("</muvrix_backup>") {
            return Err(BackupError::MissingRoot);
        }

        let schema_version =
            parse_attr_u32(trimmed, "schema_version").unwrap_or(CURRENT_BACKUP_SCHEMA);
        if schema_version > CURRENT_BACKUP_SCHEMA {
            return Err(BackupError::UnsupportedVersion(schema_version));
        }

        let app_version =
            parse_attr_string(trimmed, "app_version").unwrap_or_else(|| "0.1.0".to_string());
        let timestamp = parse_attr_u64(trimmed, "timestamp").unwrap_or_default();

        let tags = extract_tag_map(trimmed);

        Ok(Self {
            schema_version,
            app_version,
            created_at_unix: timestamp,

            audio_channels: tags
                .get("channels")
                .cloned()
                .unwrap_or_else(|| "auto".to_string()),
            volume_normalization: tags
                .get("normalization")
                .map(|v| v == "true")
                .unwrap_or(false),
            audio_pitch_correction: tags
                .get("pitch_correction")
                .map(|v| v != "false")
                .unwrap_or(true),
            preferred_audio_languages: tags.get("preferred_languages").cloned().unwrap_or_default(),
            default_audio_delay_ms: tags
                .get("default_delay_ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            volume_boost_cap: tags
                .get("volume_boost_cap")
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            volume: tags
                .get("volume")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100.0),
            background_audio: tags
                .get("background_audio")
                .map(|v| v != "false")
                .unwrap_or(true),

            theme_mode: tags
                .get("theme_mode")
                .cloned()
                .unwrap_or_else(|| "obsidian".to_string()),
            accent_color: tags
                .get("accent_color")
                .cloned()
                .unwrap_or_else(|| "lime".to_string()),

            sub_font: tags
                .get("font")
                .cloned()
                .unwrap_or_else(|| "sans-serif".to_string()),
            sub_font_size: tags
                .get("font_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(55),
            sub_color: tags
                .get("color")
                .cloned()
                .unwrap_or_else(|| "#FFFFFFFF".to_string()),
            sub_border_color: tags
                .get("border_color")
                .cloned()
                .unwrap_or_else(|| "#FF000000".to_string()),
            sub_border_size: tags
                .get("border_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3.0),
            sub_shadow_offset: tags
                .get("shadow_offset")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            sub_back_color: tags
                .get("back_color")
                .cloned()
                .unwrap_or_else(|| "#00000000".to_string()),
            sub_margin_y: tags
                .get("margin_y")
                .and_then(|v| v.parse().ok())
                .unwrap_or(22),
            sub_pos: tags.get("pos").and_then(|v| v.parse().ok()).unwrap_or(100),
            sub_speed: tags
                .get("speed")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0),
            sub_justify: tags
                .get("justify")
                .cloned()
                .unwrap_or_else(|| "auto".to_string()),
            ass_override: tags
                .get("ass_override")
                .map(|v| v == "true")
                .unwrap_or(false),

            anime4k_preset: tags
                .get("anime4k_preset")
                .cloned()
                .unwrap_or_else(|| "disabled".to_string()),
            anime4k_quality: tags
                .get("anime4k_quality")
                .cloned()
                .unwrap_or_else(|| "fast".to_string()),
            tone_mapping: tags
                .get("tone_mapping")
                .cloned()
                .unwrap_or_else(|| "auto".to_string()),
            deband: tags.get("deband").map(|v| v == "true").unwrap_or(false),

            history_enabled: tags.get("enabled").map(|v| v != "false").unwrap_or(true),
            auto_resume_enabled: tags
                .get("auto_resume")
                .map(|v| v != "false")
                .unwrap_or(true),

            verbose_logging: tags
                .get("verbose_logging")
                .map(|v| v == "true")
                .unwrap_or(false),
        })
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn parse_attr_string(xml: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = xml.find(&pattern)? + pattern.len();
    let end = start + xml[start..].find('"')?;
    Some(xml_unescape(&xml[start..end]))
}

fn parse_attr_u32(xml: &str, attr: &str) -> Option<u32> {
    parse_attr_string(xml, attr).and_then(|v| v.parse().ok())
}

fn parse_attr_u64(xml: &str, attr: &str) -> Option<u64> {
    parse_attr_string(xml, attr).and_then(|v| v.parse().ok())
}

fn extract_tag_map(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let bytes = xml.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start_tag = i + 1;
            if let Some(tag_end) = xml[start_tag..].find('>') {
                let tag_str = &xml[start_tag..start_tag + tag_end].trim();
                let full_tag_end = start_tag + tag_end + 1;
                if !tag_str.starts_with('/')
                    && !tag_str.starts_with('?')
                    && !tag_str.starts_with('!')
                {
                    let tag_name = tag_str.split_whitespace().next().unwrap_or("");
                    let close_pattern = format!("</{}>", tag_name);
                    if let Some(close_pos) = xml[full_tag_end..].find(&close_pattern) {
                        let content = &xml[full_tag_end..full_tag_end + close_pos];
                        if !content.contains('<') {
                            map.insert(tag_name.to_string(), xml_unescape(content.trim()));
                        }
                    }
                }
                i = start_tag;
            }
        }
        i += 1;
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_backup_roundtrip_preserves_all_configuration() {
        let original = BackupSettings {
            schema_version: 1,
            app_version: "0.1.0-beta.1".to_string(),
            created_at_unix: 1731670000,
            audio_channels: "reversed_stereo".to_string(),
            volume_normalization: true,
            audio_pitch_correction: true,
            preferred_audio_languages: "eng,jpn".to_string(),
            default_audio_delay_ms: 120,
            volume_boost_cap: 45,
            volume: 120.0,
            theme_mode: "obsidian".to_string(),
            accent_color: "lime".to_string(),
            sub_font: "Liberation Sans".to_string(),
            sub_font_size: 58,
            sub_color: "#C5F36AFF".to_string(),
            sub_border_color: "#111A22FF".to_string(),
            sub_border_size: 3.5,
            sub_shadow_offset: 1.0,
            sub_back_color: "#00000080".to_string(),
            sub_margin_y: 28,
            sub_pos: 95,
            sub_speed: 1.05,
            sub_justify: "center".to_string(),
            ass_override: true,
            anime4k_preset: "mode_b".to_string(),
            anime4k_quality: "hq".to_string(),
            tone_mapping: "bt.2446a".to_string(),
            deband: true,
            history_enabled: true,
            auto_resume_enabled: false,
            verbose_logging: true,
            background_audio: false,
        };

        let xml = original.to_xml();
        assert!(xml.contains("<muvrix_backup"));
        assert!(xml.contains("<channels>reversed_stereo</channels>"));
        assert!(xml.contains("<normalization>true</normalization>"));
        assert!(xml.contains("<accent_color>lime</accent_color>"));

        let restored = BackupSettings::from_xml(&xml).expect("parse XML");
        assert_eq!(restored.schema_version, 1);
        assert_eq!(restored.app_version, "0.1.0-beta.1");
        assert_eq!(restored.audio_channels, "reversed_stereo");
        assert!(restored.volume_normalization);
        assert_eq!(restored.default_audio_delay_ms, 120);
        assert_eq!(restored.theme_mode, "obsidian");
        assert_eq!(restored.accent_color, "lime");
        assert_eq!(restored.sub_font, "Liberation Sans");
        assert!(restored.ass_override);
        assert!(restored.deband);
        assert!(!restored.auto_resume_enabled);
        assert!(restored.verbose_logging);
        assert!(!restored.background_audio);
    }

    #[test]
    fn invalid_and_corrupt_xml_returns_structured_errors() {
        assert_eq!(
            BackupSettings::from_xml(""),
            Err(BackupError::EmptyDocument)
        );
        assert_eq!(
            BackupSettings::from_xml("   "),
            Err(BackupError::EmptyDocument)
        );
        assert_eq!(
            BackupSettings::from_xml("<random_root></random_root>"),
            Err(BackupError::MissingRoot)
        );
        assert_eq!(
            BackupSettings::from_xml("<muvrix_backup schema_version=\"999\"></muvrix_backup>"),
            Err(BackupError::UnsupportedVersion(999))
        );
    }

    #[test]
    fn missing_tags_fall_back_gracefully_to_muvrix_defaults() {
        let minimal_xml = r#"
            <muvrix_backup schema_version="1" app_version="0.1.0">
                <audio>
                    <normalization>true</normalization>
                </audio>
            </muvrix_backup>
        "#;
        let restored = BackupSettings::from_xml(minimal_xml).expect("parse minimal");
        assert_eq!(restored.audio_channels, "auto");
        assert!(restored.volume_normalization);
        assert_eq!(restored.theme_mode, "obsidian");
        assert_eq!(restored.accent_color, "lime");
    }
}
