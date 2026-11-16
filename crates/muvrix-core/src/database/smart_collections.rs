//! Dynamic Smart Collection data models, rule compiler, and SQL generation.

use rusqlite::ToSql;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Logical operator connecting individual rules within a smart collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RuleCombinationMode {
    #[default]
    And,
    Or,
}

impl RuleCombinationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "OR" => Self::Or,
            _ => Self::And,
        }
    }
}

/// Library metadata fields available for smart collection rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleField {
    // Text fields
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    FilePath,
    Format,
    MediaType,

    // Numeric fields
    ReleaseYear,
    Duration,
    TrackNumber,
    DiscNumber,
    FileSize,
    Bitrate,

    // Date fields
    DateAdded,
    DateModified,
    LastPlayed,

    // Library conditions
    Availability,
    WatchState,
    Favourite,
}

impl RuleField {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::AlbumArtist => "Album Artist",
            Self::Genre => "Genre",
            Self::FilePath => "File Path",
            Self::Format => "Format / Extension",
            Self::MediaType => "Media Type",
            Self::ReleaseYear => "Release Year",
            Self::Duration => "Duration (seconds)",
            Self::TrackNumber => "Track Number",
            Self::DiscNumber => "Disc Number",
            Self::FileSize => "File Size (bytes)",
            Self::Bitrate => "Bitrate (bps)",
            Self::DateAdded => "Date Added",
            Self::DateModified => "Date Modified",
            Self::LastPlayed => "Last Played",
            Self::Availability => "File Availability",
            Self::WatchState => "Watched / Played",
            Self::Favourite => "Is Favourite",
        }
    }

    /// Whitelisted SQL column or expression fragment.
    pub fn sql_column(&self) -> &'static str {
        match self {
            Self::Title => "COALESCE(mm.title, mi.file_name)",
            Self::Artist => "COALESCE(mm.artist, '')",
            Self::Album => "COALESCE(mm.album, '')",
            Self::AlbumArtist => "COALESCE(mm.album_artist, '')",
            Self::Genre => "COALESCE(mm.genre, '')",
            Self::FilePath => "mi.canonical_path",
            Self::Format => "mi.file_extension",
            Self::MediaType => "mi.media_type",
            Self::ReleaseYear => "mm.release_year",
            Self::Duration => "(mm.duration_ms / 1000)",
            Self::TrackNumber => "mm.track_number",
            Self::DiscNumber => "mm.disc_number",
            Self::FileSize => "mi.file_size",
            Self::Bitrate => "mm.overall_bitrate",
            Self::DateAdded => "mi.added_time",
            Self::DateModified => "mi.modified_time",
            Self::LastPlayed => "COALESCE(ws.updated_at, 0)",
            Self::Availability => "mi.availability_state",
            Self::WatchState => "COALESCE(ws.is_watched, 0)",
            Self::Favourite => "CASE WHEN fav.media_id IS NOT NULL THEN 1 ELSE 0 END",
        }
    }
}

/// Text comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextOperator {
    Equals,
    DoesNotEqual,
    Contains,
    DoesNotContain,
    StartsWith,
    EndsWith,
    IsEmpty,
    IsNotEmpty,
}

impl TextOperator {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Equals => "is",
            Self::DoesNotEqual => "is not",
            Self::Contains => "contains",
            Self::DoesNotContain => "does not contain",
            Self::StartsWith => "starts with",
            Self::EndsWith => "ends with",
            Self::IsEmpty => "is empty",
            Self::IsNotEmpty => "is not empty",
        }
    }
}

/// Numeric comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericOperator {
    Equals,
    GreaterThan,
    LessThan,
    AtLeast,
    AtMost,
    Between,
}

impl NumericOperator {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Equals => "equals",
            Self::GreaterThan => "greater than",
            Self::LessThan => "less than",
            Self::AtLeast => "at least",
            Self::AtMost => "at most",
            Self::Between => "between",
        }
    }
}

/// Date / time comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateOperator {
    Before,
    After,
    WithinLastNDays,
    OlderThanNDays,
    NeverPlayed,
}

impl DateOperator {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::WithinLastNDays => "in the last N days",
            Self::OlderThanNDays => "older than N days",
            Self::NeverPlayed => "never played",
        }
    }
}

/// High-level library condition operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryOperator {
    IsAvailable,
    IsMissing,
    IsPlayed,
    IsUnplayed,
    IsFavourite,
    RecentlyAddedNDays,
}

impl LibraryOperator {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::IsAvailable => "file is available",
            Self::IsMissing => "file is missing / broken",
            Self::IsPlayed => "is played / watched",
            Self::IsUnplayed => "is unplayed",
            Self::IsFavourite => "is in favourites",
            Self::RecentlyAddedNDays => "recently added in last N days",
        }
    }
}

/// Strongly-typed individual smart rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SmartRule {
    Text {
        field: RuleField,
        operator: TextOperator,
        value: String,
    },
    Numeric {
        field: RuleField,
        operator: NumericOperator,
        value: f64,
        value_to: Option<f64>,
    },
    Date {
        field: RuleField,
        operator: DateOperator,
        timestamp: Option<i64>,
        days: Option<i64>,
    },
    Library {
        operator: LibraryOperator,
        days: Option<i64>,
    },
}

/// The collection of rules defining a Smart Collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RuleDefinition {
    pub mode: RuleCombinationMode,
    pub rules: Vec<SmartRule>,
}

impl RuleDefinition {
    pub fn new(mode: RuleCombinationMode) -> Self {
        Self {
            mode,
            rules: Vec::new(),
        }
    }

    pub fn add_rule(&mut self, rule: SmartRule) {
        self.rules.push(rule);
    }
}

/// Persisted smart collection entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartCollection {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub rules: RuleDefinition,
    pub combination_mode: RuleCombinationMode,
    pub sort_field: Option<String>,
    pub sort_direction: Option<String>,
    pub display_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Result of compiling a `RuleDefinition` into a parameterized SQL condition.
pub struct CompiledRuleQuery {
    pub where_clause: String,
    pub params: Vec<Box<dyn ToSql>>,
}

/// Compiles a set of smart collection rules into a parameterized SQL fragment and bound parameters.
///
/// Guaranteed properties:
/// 1. Strict allowlists: Column expressions and operators are derived exclusively from safe enums.
/// 2. SQL injection resistance: User values are strictly passed as parameterized bindings (`?`).
/// 3. Complexity limits: Caps maximum condition rules at 20.
pub fn compile_smart_rules(definition: &RuleDefinition) -> Result<CompiledRuleQuery, String> {
    if definition.rules.is_empty() {
        return Ok(CompiledRuleQuery {
            where_clause: "1=1".to_string(),
            params: Vec::new(),
        });
    }

    if definition.rules.len() > 20 {
        return Err("Maximum number of rules exceeded (limit: 20)".to_string());
    }

    let mut clauses = Vec::new();
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    let now = current_timestamp();

    for rule in &definition.rules {
        match rule {
            SmartRule::Text {
                field,
                operator,
                value,
            } => {
                let col = field.sql_column();
                match operator {
                    TextOperator::Equals => {
                        clauses.push(format!("LOWER({col}) = LOWER(?)"));
                        params.push(Box::new(value.clone()));
                    }
                    TextOperator::DoesNotEqual => {
                        clauses.push(format!("(LOWER({col}) != LOWER(?) OR {col} IS NULL)"));
                        params.push(Box::new(value.clone()));
                    }
                    TextOperator::Contains => {
                        clauses.push(format!("{col} LIKE ?"));
                        params.push(Box::new(format!("%{value}%")));
                    }
                    TextOperator::DoesNotContain => {
                        clauses.push(format!("({col} NOT LIKE ? OR {col} IS NULL)"));
                        params.push(Box::new(format!("%{value}%")));
                    }
                    TextOperator::StartsWith => {
                        clauses.push(format!("{col} LIKE ?"));
                        params.push(Box::new(format!("{value}%")));
                    }
                    TextOperator::EndsWith => {
                        clauses.push(format!("{col} LIKE ?"));
                        params.push(Box::new(format!("%{value}")));
                    }
                    TextOperator::IsEmpty => {
                        clauses.push(format!("({col} IS NULL OR TRIM({col}) = '')"));
                    }
                    TextOperator::IsNotEmpty => {
                        clauses.push(format!("({col} IS NOT NULL AND TRIM({col}) != '')"));
                    }
                }
            }
            SmartRule::Numeric {
                field,
                operator,
                value,
                value_to,
            } => {
                let col = field.sql_column();
                let v = *value as i64;
                match operator {
                    NumericOperator::Equals => {
                        clauses.push(format!("{col} = ?"));
                        params.push(Box::new(v));
                    }
                    NumericOperator::GreaterThan => {
                        clauses.push(format!("{col} > ?"));
                        params.push(Box::new(v));
                    }
                    NumericOperator::LessThan => {
                        clauses.push(format!("{col} < ?"));
                        params.push(Box::new(v));
                    }
                    NumericOperator::AtLeast => {
                        clauses.push(format!("{col} >= ?"));
                        params.push(Box::new(v));
                    }
                    NumericOperator::AtMost => {
                        clauses.push(format!("{col} <= ?"));
                        params.push(Box::new(v));
                    }
                    NumericOperator::Between => {
                        let to = value_to.unwrap_or(*value) as i64;
                        clauses.push(format!("{col} BETWEEN ? AND ?"));
                        params.push(Box::new(v.min(to)));
                        params.push(Box::new(v.max(to)));
                    }
                }
            }
            SmartRule::Date {
                field,
                operator,
                timestamp,
                days,
            } => {
                let col = field.sql_column();
                match operator {
                    DateOperator::Before => {
                        let ts = timestamp.unwrap_or(now);
                        clauses.push(format!("({col} < ? AND {col} > 0)"));
                        params.push(Box::new(ts));
                    }
                    DateOperator::After => {
                        let ts = timestamp.unwrap_or(now);
                        clauses.push(format!("{col} > ?"));
                        params.push(Box::new(ts));
                    }
                    DateOperator::WithinLastNDays => {
                        let d = days.unwrap_or(30).max(1);
                        let threshold = now.saturating_sub(d * 86400);
                        clauses.push(format!("{col} >= ?"));
                        params.push(Box::new(threshold));
                    }
                    DateOperator::OlderThanNDays => {
                        let d = days.unwrap_or(30).max(1);
                        let threshold = now.saturating_sub(d * 86400);
                        clauses.push(format!("({col} < ? AND {col} > 0)"));
                        params.push(Box::new(threshold));
                    }
                    DateOperator::NeverPlayed => {
                        clauses.push(
                            "(ws.is_watched IS NULL OR ws.is_watched = 0 OR ws.updated_at = 0)"
                                .to_string(),
                        );
                    }
                }
            }
            SmartRule::Library { operator, days } => match operator {
                LibraryOperator::IsAvailable => {
                    clauses.push("mi.availability_state = 'available'".to_string());
                }
                LibraryOperator::IsMissing => {
                    clauses.push("mi.availability_state = 'unavailable'".to_string());
                }
                LibraryOperator::IsPlayed => {
                    clauses.push("COALESCE(ws.is_watched, 0) = 1".to_string());
                }
                LibraryOperator::IsUnplayed => {
                    clauses.push("COALESCE(ws.is_watched, 0) = 0".to_string());
                }
                LibraryOperator::IsFavourite => {
                    clauses.push("fav.media_id IS NOT NULL".to_string());
                }
                LibraryOperator::RecentlyAddedNDays => {
                    let d = days.unwrap_or(30).max(1);
                    let threshold = now.saturating_sub(d * 86400);
                    clauses.push("mi.added_time >= ?".to_string());
                    params.push(Box::new(threshold));
                }
            },
        }
    }

    let joiner = match definition.mode {
        RuleCombinationMode::And => " AND ",
        RuleCombinationMode::Or => " OR ",
    };

    let joined = clauses
        .into_iter()
        .map(|c| format!("({c})"))
        .collect::<Vec<_>>()
        .join(joiner);

    Ok(CompiledRuleQuery {
        where_clause: joined,
        params,
    })
}
