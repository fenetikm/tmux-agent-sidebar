//! Custom bottom panel: row types, NDJSON parsing, command execution and
//! the per-process result cache.
//!
//! This module is deliberately free of ratatui, tmux and `AppState` types so
//! parsing and colour naming can be unit-tested without rendering a frame.
//! Mapping a [`PanelColor`] onto a concrete terminal colour is the UI
//! layer's job (`src/ui/bottom/panel.rs`).

use unicode_width::UnicodeWidthChar;

/// Maximum display width of a row's leading icon. Wider values are
/// truncated so a stray emoji cannot shift the text column.
const ICON_MAX_WIDTH: usize = 2;

/// Colour names a panel script may use. Deliberately a small closed
/// vocabulary: the concrete colour comes from the active theme, so panels
/// follow the user's palette instead of hardcoding 256-colour indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelColor {
    #[default]
    Default,
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

impl PanelColor {
    /// Map a script-supplied name onto the vocabulary. Unrecognised names
    /// fall back to [`PanelColor::Default`] rather than failing the row —
    /// a typo should not blank the panel.
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "muted" => Self::Muted,
            "accent" => Self::Accent,
            "success" => Self::Success,
            "warning" => Self::Warning,
            "danger" => Self::Danger,
            _ => Self::Default,
        }
    }
}

/// One rendered line of panel output.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelRow {
    pub text: String,
    pub text_color: PanelColor,
    pub icon: Option<String>,
    pub icon_color: PanelColor,
    pub url: Option<String>,
    pub heading: bool,
}

/// Truncate an icon to [`ICON_MAX_WIDTH`] display cells, never splitting a
/// wide glyph in half.
fn truncate_icon(icon: &str) -> Option<String> {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in icon.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > ICON_MAX_WIDTH {
            break;
        }
        width += w;
        out.push(ch);
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Parse NDJSON panel output into rows.
///
/// Each line is parsed independently so one malformed row is skipped rather
/// than discarding the whole payload. Returns the rows alongside an optional
/// error, which is set only when every line failed to parse — a script
/// emitting plain text should not render as a silently blank panel.
pub fn parse_rows(stdout: &str) -> (Vec<PanelRow>, Option<String>) {
    let mut rows = Vec::new();
    let mut unparsable = 0usize;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            unparsable += 1;
            continue;
        };
        let Some(obj) = value.as_object() else {
            unparsable += 1;
            continue;
        };
        // A row without text has nothing to render; skip it quietly.
        let Some(text) = obj.get("text").and_then(|v| v.as_str()) else {
            continue;
        };

        let text_color = obj
            .get("text_color")
            .and_then(|v| v.as_str())
            .map(PanelColor::from_name)
            .unwrap_or_default();
        let heading = obj
            .get("heading")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // Headings are structural labels, not targets: an icon column would
        // break the alignment they exist to organise.
        let (icon, url) = if heading {
            (None, None)
        } else {
            (
                obj.get("icon")
                    .and_then(|v| v.as_str())
                    .and_then(truncate_icon),
                obj.get("url")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            )
        };
        let icon_color = obj
            .get("icon_color")
            .and_then(|v| v.as_str())
            .map(PanelColor::from_name)
            .unwrap_or(text_color);

        rows.push(PanelRow {
            text: text.to_string(),
            text_color,
            icon,
            icon_color,
            url,
            heading,
        });
    }

    let error = if rows.is_empty() && unparsable > 0 {
        Some(format!("no valid rows ({unparsable} unparsable lines)"))
    } else {
        None
    };
    (rows, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_row() {
        // r##"..."## because the payload itself contains `"#`, which would
        // close a plain r#"..."# raw string.
        let (rows, err) = parse_rows(
            r##"{"text":"#412 fix","text_color":"warning","icon":"!","icon_color":"danger","url":"https://x/1"}"##,
        );
        assert!(err.is_none());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "#412 fix");
        assert_eq!(rows[0].text_color, PanelColor::Warning);
        assert_eq!(rows[0].icon.as_deref(), Some("!"));
        assert_eq!(rows[0].icon_color, PanelColor::Danger);
        assert_eq!(rows[0].url.as_deref(), Some("https://x/1"));
        assert!(!rows[0].heading);
    }

    #[test]
    fn text_only_row_uses_defaults() {
        let (rows, err) = parse_rows(r#"{"text":"hello"}"#);
        assert!(err.is_none());
        assert_eq!(rows[0].text_color, PanelColor::Default);
        assert_eq!(rows[0].icon, None);
        assert_eq!(rows[0].url, None);
        assert!(!rows[0].heading);
    }

    #[test]
    fn icon_color_defaults_to_text_color() {
        let (rows, _) = parse_rows(r#"{"text":"a","text_color":"success","icon":"*"}"#);
        assert_eq!(rows[0].icon_color, PanelColor::Success);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let (rows, err) = parse_rows(r#"{"text":"a","nonsense":42,"deep":{"x":1}}"#);
        assert!(err.is_none());
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn unknown_color_falls_back_to_default() {
        let (rows, _) = parse_rows(r#"{"text":"a","text_color":"chartreuse"}"#);
        assert_eq!(rows[0].text_color, PanelColor::Default);
    }

    #[test]
    fn row_without_text_is_skipped() {
        let (rows, err) = parse_rows("{\"text\":\"a\"}\n{\"url\":\"https://x\"}\n{\"text\":\"b\"}");
        assert_eq!(rows.len(), 2);
        assert!(err.is_none(), "a skipped row is not an error");
    }

    #[test]
    fn headings_ignore_icon_and_url() {
        let (rows, _) =
            parse_rows(r#"{"text":"Needs review","heading":true,"icon":"x","url":"https://x"}"#);
        assert!(rows[0].heading);
        assert_eq!(rows[0].icon, None);
        assert_eq!(rows[0].url, None);
    }

    #[test]
    fn blank_lines_are_ignored() {
        let (rows, err) = parse_rows("{\"text\":\"a\"}\n\n   \n{\"text\":\"b\"}\n");
        assert_eq!(rows.len(), 2);
        assert!(err.is_none());
    }

    #[test]
    fn one_malformed_line_is_skipped() {
        let (rows, err) = parse_rows("{\"text\":\"a\"}\nnot json\n{\"text\":\"b\"}");
        assert_eq!(rows.len(), 2);
        assert!(err.is_none(), "partial success is not an error");
    }

    #[test]
    fn all_lines_malformed_is_an_error() {
        let (rows, err) = parse_rows("not json\nalso not json");
        assert!(rows.is_empty());
        assert_eq!(err.as_deref(), Some("no valid rows (2 unparsable lines)"));
    }

    #[test]
    fn empty_output_is_not_an_error() {
        let (rows, err) = parse_rows("");
        assert!(rows.is_empty());
        assert!(err.is_none());
    }

    #[test]
    fn icon_is_truncated_to_two_display_cells() {
        let (rows, _) = parse_rows(r#"{"text":"a","icon":"abc"}"#);
        assert_eq!(rows[0].icon.as_deref(), Some("ab"));
    }

    #[test]
    fn wide_icon_glyph_is_kept_whole() {
        // A single emoji is two display cells wide: it fits exactly.
        let (rows, _) = parse_rows(r#"{"text":"a","icon":"🎉"}"#);
        assert_eq!(rows[0].icon.as_deref(), Some("🎉"));
    }

    #[test]
    fn two_wide_glyphs_are_truncated_to_one() {
        let (rows, _) = parse_rows(r#"{"text":"a","icon":"🎉🎉"}"#);
        assert_eq!(rows[0].icon.as_deref(), Some("🎉"));
    }

    #[test]
    fn color_from_name_covers_the_vocabulary() {
        assert_eq!(PanelColor::from_name("default"), PanelColor::Default);
        assert_eq!(PanelColor::from_name("muted"), PanelColor::Muted);
        assert_eq!(PanelColor::from_name("accent"), PanelColor::Accent);
        assert_eq!(PanelColor::from_name("success"), PanelColor::Success);
        assert_eq!(PanelColor::from_name("warning"), PanelColor::Warning);
        assert_eq!(PanelColor::from_name("danger"), PanelColor::Danger);
        assert_eq!(PanelColor::from_name("DANGER"), PanelColor::Danger);
        assert_eq!(PanelColor::from_name(""), PanelColor::Default);
    }
}
