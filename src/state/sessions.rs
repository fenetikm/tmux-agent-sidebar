use std::collections::HashMap;

use crate::tmux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionsPanelHeight {
    Hidden,
    Auto,
    Fixed(u16),
}

/// Read `@sidebar_sessions_height` from tmux global options.
///
/// - unset / empty / invalid → `Auto`
/// - `"0"` → `Hidden`
/// - `"auto"` (case-insensitive) → `Auto`
/// - positive integer → `Fixed(n)`
pub fn sessions_panel_height_from_options(opts: &HashMap<String, String>) -> SessionsPanelHeight {
    let Some(raw) = opts.get(tmux::SIDEBAR_SESSIONS_HEIGHT) else {
        return SessionsPanelHeight::Auto;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SessionsPanelHeight::Auto;
    }
    if trimmed.eq_ignore_ascii_case("auto") {
        return SessionsPanelHeight::Auto;
    }
    if trimmed == "0" {
        return SessionsPanelHeight::Hidden;
    }
    if let Ok(n) = trimmed.parse::<u16>() {
        return SessionsPanelHeight::Fixed(n);
    }
    SessionsPanelHeight::Auto
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn opts(key: &str, val: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(key.into(), val.into());
        m
    }

    #[test]
    fn height_defaults_to_auto() {
        assert_eq!(
            sessions_panel_height_from_options(&HashMap::new()),
            SessionsPanelHeight::Auto
        );
    }

    #[test]
    fn height_zero_is_hidden() {
        let o = opts(crate::tmux::SIDEBAR_SESSIONS_HEIGHT, "0");
        assert_eq!(
            sessions_panel_height_from_options(&o),
            SessionsPanelHeight::Hidden
        );
    }

    #[test]
    fn height_auto_case_insensitive() {
        for v in ["auto", "AUTO", " Auto "] {
            let o = opts(crate::tmux::SIDEBAR_SESSIONS_HEIGHT, v);
            assert_eq!(
                sessions_panel_height_from_options(&o),
                SessionsPanelHeight::Auto
            );
        }
    }

    #[test]
    fn height_fixed_parses_integer() {
        let o = opts(crate::tmux::SIDEBAR_SESSIONS_HEIGHT, "3");
        assert_eq!(
            sessions_panel_height_from_options(&o),
            SessionsPanelHeight::Fixed(3)
        );
    }

    #[test]
    fn height_invalid_falls_back_to_auto() {
        let o = opts(crate::tmux::SIDEBAR_SESSIONS_HEIGHT, "abc");
        assert_eq!(
            sessions_panel_height_from_options(&o),
            SessionsPanelHeight::Auto
        );
    }
}
