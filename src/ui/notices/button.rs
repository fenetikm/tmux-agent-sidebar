use ratatui::{style::Style, text::Span};

use crate::state::AppState;
use crate::tmux::CODEX_AGENT;

/// Width (in columns) reserved for the notices indicator button in the
/// secondary header: the glyph plus a trailing space.
pub(in crate::ui) const BUTTON_WIDTH: usize = 2;

/// Whether the missing-hooks section should render a `[copy]` button
/// next to `agent`. Only Codex qualifies — Claude's setup story is
/// owned by the dedicated `Plugin / claude` section (which has its own
/// `[prompt]` button), so adding a second clickable copy target on the
/// Claude row would race with it and flip the shared `[copied]` feedback
/// state for both buttons at once.
///
/// Kept as a pure check so layout calculations do not pay the cost of
/// resolving the running binary path on every frame.
pub(super) fn missing_hooks_has_copy_button(agent: &str) -> bool {
    agent == CODEX_AGENT
}

/// Span for the notices indicator glyph. Always rendered in the waiting
/// (yellow) color so it reads as an information badge.
pub(in crate::ui) fn button_span<'a>(state: &AppState) -> Span<'a> {
    Span::styled("ⓘ", Style::default().fg(state.theme.status_waiting))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::NoticesMissingHookGroup;

    fn state_with(version: Option<(&str, &str)>, groups: Vec<(&str, Vec<&str>)>) -> AppState {
        let mut state = AppState::new(String::new());
        state.version_notice = version.map(|(local, latest)| crate::version::UpdateNotice {
            local_version: local.into(),
            latest_version: latest.into(),
        });
        state.notices.missing_hook_groups = groups
            .into_iter()
            .map(|(agent, hooks)| NoticesMissingHookGroup {
                agent: agent.into(),
                hooks: hooks.into_iter().map(String::from).collect(),
            })
            .collect();
        state
    }

    #[test]
    fn missing_hooks_has_copy_button_only_for_codex() {
        // Claude is excluded because the Plugin / claude section owns
        // its own [prompt] button — leaving a [copy] on the Claude row
        // would race with it on the shared `[copied]` feedback state.
        assert!(missing_hooks_has_copy_button("codex"));
        assert!(!missing_hooks_has_copy_button("claude"));
        assert!(!missing_hooks_has_copy_button("gemini"));
        assert!(!missing_hooks_has_copy_button(""));
    }

    // ─── has_info branches ───────────────────────────────────────────

    #[test]
    fn has_info_false_when_no_version_and_no_hooks() {
        let state = state_with(None, vec![]);
        assert!(!state.has_notices_header());
    }

    #[test]
    fn has_info_true_when_only_version_notice() {
        let state = state_with(Some(("0.2.6", "0.2.7")), vec![]);
        assert!(state.has_notices_header());
    }

    #[test]
    fn has_info_true_when_only_missing_hooks() {
        let state = state_with(None, vec![("claude", vec!["Stop"])]);
        assert!(state.has_notices_header());
    }

    #[test]
    fn has_info_true_when_both_version_and_hooks() {
        let state = state_with(Some(("0.2.6", "0.2.7")), vec![("claude", vec!["Stop"])]);
        assert!(state.has_notices_header());
    }

    #[test]
    fn has_info_true_when_only_plugin_notice() {
        let mut state = state_with(None, vec![]);
        state.notices.claude_plugin_notice =
            Some(crate::state::ClaudePluginNotice::InstallRecommended);
        assert!(state.has_notices_header());
    }

    // ─── button_span style ───────────────────────────────────────────

    #[test]
    fn button_span_uses_waiting_color_and_info_glyph() {
        let state = AppState::new(String::new());
        let span = button_span(&state);
        assert_eq!(span.content.as_ref(), "ⓘ");
        assert_eq!(span.style.fg, Some(state.theme.status_waiting));
    }

    #[test]
    fn button_width_reserves_two_columns() {
        assert_eq!(BUTTON_WIDTH, 2);
    }
}
