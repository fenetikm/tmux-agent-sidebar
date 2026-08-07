use std::collections::HashMap;

use crate::group::RepoGroup;
use crate::state::ScrollState;
use crate::tmux;

/// Minimum agent list rows reserved when auto-sizing the sessions panel.
const MIN_AGENT_ROWS: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionsPanelHeight {
    Hidden,
    Auto,
    Fixed(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub tmux_session: String,
    pub agent_count: usize,
    pub has_attention: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone)]
pub struct SessionsPanelState {
    pub rows: Vec<SessionRow>,
    pub scroll: ScrollState,
    pub height_mode: SessionsPanelHeight,
    pub current_tmux_session: String,
}

impl Default for SessionsPanelState {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            scroll: ScrollState::default(),
            height_mode: SessionsPanelHeight::Auto,
            current_tmux_session: String::new(),
        }
    }
}

impl SessionsPanelState {
    pub fn refresh_rows(&mut self, repo_groups: &[RepoGroup]) {
        let mut counts: HashMap<String, (usize, bool)> = HashMap::new();

        for group in repo_groups {
            for (pane, _) in &group.panes {
                if pane.tmux_session.is_empty() {
                    continue;
                }
                let entry = counts
                    .entry(pane.tmux_session.clone())
                    .or_insert((0, false));
                entry.0 += 1;
                if pane.attention {
                    entry.1 = true;
                }
            }
        }

        let mut rows: Vec<SessionRow> = counts
            .into_iter()
            .map(|(tmux_session, (agent_count, has_attention))| SessionRow {
                tmux_session: tmux_session.clone(),
                agent_count,
                has_attention,
                is_current: tmux_session == self.current_tmux_session,
            })
            .collect();

        rows.sort_by(|a, b| a.tmux_session.cmp(&b.tmux_session));
        self.rows = rows;
    }

    pub fn effective_content_height(
        &self,
        term_height: u16,
        bottom_panel_height: u16,
        pet_band_height: u16,
    ) -> u16 {
        if self.rows.len() <= 1 || self.height_mode == SessionsPanelHeight::Hidden {
            return 0;
        }

        let row_count = self.rows.len() as u16;
        match self.height_mode {
            SessionsPanelHeight::Hidden => 0,
            SessionsPanelHeight::Fixed(n) => n.min(row_count),
            SessionsPanelHeight::Auto => {
                let max_content = term_height
                    .saturating_sub(bottom_panel_height)
                    .saturating_sub(pet_band_height)
                    .saturating_sub(1) // divider
                    .saturating_sub(MIN_AGENT_ROWS);
                row_count.min(max_content)
            }
        }
    }

    /// Total top band including the 1-row divider. 0 when hidden.
    pub fn total_band_height(
        &self,
        term_height: u16,
        bottom_panel_height: u16,
        pet_band_height: u16,
    ) -> u16 {
        let content =
            self.effective_content_height(term_height, bottom_panel_height, pet_band_height);
        if content > 0 { content + 1 } else { 0 }
    }
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
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::tmux::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorktreeMetadata};
    use std::collections::HashMap;

    fn opts(key: &str, val: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(key.into(), val.into());
        m
    }

    fn pane(id: &str, tmux_session: &str, attention: bool) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status: PaneStatus::Running,
            attention,
            agent: AgentType::Claude,
            path: "/tmp".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            tmux_session: tmux_session.into(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
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

    #[test]
    fn refresh_rows_groups_by_tmux_session() {
        let mut state = SessionsPanelState::default();
        state.current_tmux_session = "main".into();
        let groups = vec![RepoGroup {
            name: "proj".into(),
            has_focus: false,
            panes: vec![
                (pane("%1", "main", false), PaneGitInfo::default()),
                (pane("%2", "work", true), PaneGitInfo::default()),
                (pane("%3", "work", false), PaneGitInfo::default()),
            ],
        }];
        state.refresh_rows(&groups);
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].tmux_session, "main");
        assert_eq!(state.rows[0].agent_count, 1);
        assert_eq!(state.rows[0].is_current, true);
        assert_eq!(state.rows[1].tmux_session, "work");
        assert_eq!(state.rows[1].agent_count, 2);
        assert_eq!(state.rows[1].has_attention, true);
    }

    #[test]
    fn refresh_rows_skips_empty_tmux_session_keys() {
        let mut state = SessionsPanelState::default();
        let groups = vec![RepoGroup {
            name: "proj".into(),
            has_focus: false,
            panes: vec![
                (pane("%1", "", false), PaneGitInfo::default()),
                (pane("%2", "work", false), PaneGitInfo::default()),
            ],
        }];
        state.refresh_rows(&groups);
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].tmux_session, "work");
    }

    #[test]
    fn effective_height_zero_for_single_session() {
        let mut state = SessionsPanelState::default();
        state.height_mode = SessionsPanelHeight::Auto;
        state.rows = vec![SessionRow {
            tmux_session: "main".into(),
            agent_count: 1,
            has_attention: false,
            is_current: true,
        }];
        assert_eq!(state.effective_content_height(40, 20, 1), 0);
    }

    #[test]
    fn auto_cap_leaves_five_agent_rows() {
        let mut state = SessionsPanelState::default();
        state.height_mode = SessionsPanelHeight::Auto;
        state.rows = (0..10)
            .map(|i| SessionRow {
                tmux_session: format!("s{i}"),
                agent_count: 1,
                has_attention: false,
                is_current: i == 0,
            })
            .collect();
        // term=20, bottom=10, pet=1, divider=1 → max sessions = 20-10-1-1-5 = 3
        assert_eq!(state.effective_content_height(20, 10, 1), 3);
    }

    #[test]
    fn total_band_height_includes_divider_when_visible() {
        let mut state = SessionsPanelState::default();
        state.height_mode = SessionsPanelHeight::Fixed(2);
        state.rows = vec![
            SessionRow {
                tmux_session: "main".into(),
                agent_count: 1,
                has_attention: false,
                is_current: true,
            },
            SessionRow {
                tmux_session: "work".into(),
                agent_count: 1,
                has_attention: false,
                is_current: false,
            },
        ];
        assert_eq!(state.total_band_height(40, 10, 1), 3);
    }

    #[test]
    fn total_band_height_zero_when_hidden() {
        let mut state = SessionsPanelState::default();
        state.height_mode = SessionsPanelHeight::Auto;
        state.rows = vec![SessionRow {
            tmux_session: "main".into(),
            agent_count: 1,
            has_attention: false,
            is_current: true,
        }];
        assert_eq!(state.total_band_height(40, 10, 1), 0);
    }
}
