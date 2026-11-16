//! Full-text search query sanitization, tokenization and FTS5 integration.

/// Sanitizes user input into a safe and efficient SQLite FTS5 prefix search query.
///
/// Strips syntax punctuation that causes FTS5 syntax errors while preserving Unicode
/// alphanumeric characters, hyphens, and underscores. Formats each token as a quoted
/// prefix match (`"token"*`) for instant search-as-you-type behavior.
pub fn sanitize_fts5_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut tokens = Vec::new();
    for word in trimmed.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();

        // Must contain at least one alphanumeric character to be a valid search term
        if clean.chars().any(|c| c.is_alphanumeric()) {
            // Quote the token to escape any FTS5 reserved keywords (AND, OR, NOT, NEAR)
            // and append '*' for instant search-as-you-type prefix completion.
            tokens.push(format!("\"{}\"*", clean));
        }
    }

    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_empty_and_whitespace() {
        assert_eq!(sanitize_fts5_query(""), "");
        assert_eq!(sanitize_fts5_query("   "), "");
        assert_eq!(sanitize_fts5_query("\t\n"), "");
    }

    #[test]
    fn test_sanitize_standard_alphanumeric() {
        assert_eq!(sanitize_fts5_query("Matrix"), "\"Matrix\"*");
        assert_eq!(
            sanitize_fts5_query("The Dark Knight"),
            "\"The\"* \"Dark\"* \"Knight\"*"
        );
    }

    #[test]
    fn test_sanitize_reserved_fts5_keywords() {
        assert_eq!(sanitize_fts5_query("NOT"), "\"NOT\"*");
        assert_eq!(sanitize_fts5_query("AND"), "\"AND\"*");
        assert_eq!(sanitize_fts5_query("OR"), "\"OR\"*");
        assert_eq!(sanitize_fts5_query("NEAR"), "\"NEAR\"*");
    }

    #[test]
    fn test_sanitize_special_characters_and_punctuation() {
        assert_eq!(
            sanitize_fts5_query("Blade Runner 2049 (Final Cut) [1080p]!"),
            "\"Blade\"* \"Runner\"* \"2049\"* \"Final\"* \"Cut\"* \"1080p\"*"
        );
        assert_eq!(
            sanitize_fts5_query("quotes: \"hello\" *asterisk*"),
            "\"quotes\"* \"hello\"* \"asterisk\"*"
        );
        assert_eq!(sanitize_fts5_query("??? !!! ###"), "");
    }

    #[test]
    fn test_sanitize_unicode_support() {
        assert_eq!(
            sanitize_fts5_query("君の名は Your Name"),
            "\"君の名は\"* \"Your\"* \"Name\"*"
        );
        assert_eq!(
            sanitize_fts5_query("Сталкер Stalker"),
            "\"Сталкер\"* \"Stalker\"*"
        );
        assert_eq!(
            sanitize_fts5_query("Amélie Poulain"),
            "\"Amélie\"* \"Poulain\"*"
        );
    }

    #[test]
    fn test_sanitize_hyphens_and_underscores() {
        assert_eq!(
            sanitize_fts5_query("sci-fi track_01"),
            "\"sci-fi\"* \"track_01\"*"
        );
    }
}
