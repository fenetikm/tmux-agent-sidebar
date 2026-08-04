use std::collections::HashMap;

use crate::tmux::{self, PaneStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIcons {
    /// Icon for the "All" filter in the top filter bar.
    all: String,
    running: String,
    background: String,
    waiting: String,
    idle: String,
    error: String,
    unknown: String,
    /// Per-provider glyphs. Only compact rows render these; expanded rows
    /// carry provider identity in the title colour instead.
    agent_claude: String,
    agent_codex: String,
    agent_opencode: String,
    agent_cursor: String,
    agent_unknown: String,
}

impl Default for StatusIcons {
    fn default() -> Self {
        Self {
            all: "≡".into(),
            running: "●".into(),
            background: "◎".into(),
            waiting: "◐".into(),
            idle: "○".into(),
            error: "✕".into(),
            unknown: "·".into(),
            agent_claude: "✳".into(),
            agent_codex: "◆".into(),
            agent_opencode: "◇".into(),
            agent_cursor: "◈".into(),
            agent_unknown: "·".into(),
        }
    }
}

impl StatusIcons {
    /// Load status icons from tmux @sidebar_icon_* variables, falling back to defaults.
    pub fn from_tmux() -> Self {
        let all_opts = tmux::get_all_global_options();
        Self::from_options(&all_opts)
    }

    pub fn from_options(all_opts: &HashMap<String, String>) -> Self {
        let mut icons = Self::default();

        let read = |var: &str, fallback: &str| -> String {
            all_opts
                .get(var)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| fallback.to_string())
        };

        icons.all = read(tmux::SIDEBAR_ICON_ALL, &icons.all);
        icons.running = read(tmux::SIDEBAR_ICON_RUNNING, &icons.running);
        icons.background = read(tmux::SIDEBAR_ICON_BACKGROUND, &icons.background);
        icons.waiting = read(tmux::SIDEBAR_ICON_WAITING, &icons.waiting);
        icons.idle = read(tmux::SIDEBAR_ICON_IDLE, &icons.idle);
        icons.error = read(tmux::SIDEBAR_ICON_ERROR, &icons.error);
        icons.unknown = read(tmux::SIDEBAR_ICON_UNKNOWN, &icons.unknown);
        let read_agent = |var: &str, fallback: &str| -> String {
            match all_opts.get(var) {
                None => fallback.to_string(),
                Some(s) if s.is_empty() => String::new(),
                Some(s) if s.trim().is_empty() => fallback.to_string(),
                Some(s) => s.trim().to_string(),
            }
        };
        icons.agent_claude = read_agent(tmux::SIDEBAR_ICON_AGENT_CLAUDE, &icons.agent_claude);
        icons.agent_codex = read_agent(tmux::SIDEBAR_ICON_AGENT_CODEX, &icons.agent_codex);
        icons.agent_opencode = read_agent(tmux::SIDEBAR_ICON_AGENT_OPENCODE, &icons.agent_opencode);
        icons.agent_cursor = read_agent(tmux::SIDEBAR_ICON_AGENT_CURSOR, &icons.agent_cursor);
        icons.agent_unknown = read_agent(tmux::SIDEBAR_ICON_AGENT_UNKNOWN, &icons.agent_unknown);
        icons
    }

    /// Icon used for the "All" filter (not tied to any PaneStatus).
    pub fn all_icon(&self) -> &str {
        self.all.as_str()
    }

    pub fn status_icon(&self, status: &PaneStatus) -> &str {
        match status {
            PaneStatus::Running => self.running.as_str(),
            PaneStatus::Background => self.background.as_str(),
            PaneStatus::Waiting => self.waiting.as_str(),
            PaneStatus::Idle => self.idle.as_str(),
            PaneStatus::Error => self.error.as_str(),
            PaneStatus::Unknown => self.unknown.as_str(),
        }
    }

    /// Glyph identifying which agent owns a pane. `AgentType::Unknown` is
    /// currently unreachable because `AgentType::from_label` returns `None`
    /// for unrecognised labels, but it carries a glyph so the match stays
    /// exhaustive if that changes.
    pub fn agent_icon(&self, agent: &tmux::AgentType) -> &str {
        match agent {
            tmux::AgentType::Claude => self.agent_claude.as_str(),
            tmux::AgentType::Codex => self.agent_codex.as_str(),
            tmux::AgentType::OpenCode => self.agent_opencode.as_str(),
            tmux::AgentType::Cursor => self.agent_cursor.as_str(),
            tmux::AgentType::Unknown => self.agent_unknown.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icons_match_current_glyphs() {
        let icons = StatusIcons::default();
        assert_eq!(icons.all_icon(), "≡");
        assert_eq!(icons.status_icon(&PaneStatus::Running), "●");
        assert_eq!(icons.status_icon(&PaneStatus::Background), "◎");
        assert_eq!(icons.status_icon(&PaneStatus::Waiting), "◐");
        assert_eq!(icons.status_icon(&PaneStatus::Idle), "○");
        assert_eq!(icons.status_icon(&PaneStatus::Error), "✕");
        assert_eq!(icons.status_icon(&PaneStatus::Unknown), "·");
    }

    #[test]
    fn tmux_options_override_defaults() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_ICON_ALL.into(), "∀".into());
        opts.insert(tmux::SIDEBAR_ICON_RUNNING.into(), "◉".into());
        opts.insert(tmux::SIDEBAR_ICON_BACKGROUND.into(), "⊙".into());
        opts.insert(tmux::SIDEBAR_ICON_UNKNOWN.into(), "∎".into());

        let icons = StatusIcons::from_options(&opts);
        assert_eq!(icons.all_icon(), "∀");
        assert_eq!(icons.status_icon(&PaneStatus::Running), "◉");
        assert_eq!(icons.status_icon(&PaneStatus::Background), "⊙");
        assert_eq!(icons.status_icon(&PaneStatus::Unknown), "∎");
        assert_eq!(icons.status_icon(&PaneStatus::Waiting), "◐");
    }

    #[test]
    fn default_agent_icons_match_current_glyphs() {
        let icons = StatusIcons::default();
        assert_eq!(icons.agent_icon(&tmux::AgentType::Claude), "✳");
        assert_eq!(icons.agent_icon(&tmux::AgentType::Codex), "◆");
        assert_eq!(icons.agent_icon(&tmux::AgentType::OpenCode), "◇");
        assert_eq!(icons.agent_icon(&tmux::AgentType::Cursor), "◈");
        assert_eq!(icons.agent_icon(&tmux::AgentType::Unknown), "·");
    }

    #[test]
    fn tmux_options_override_agent_icons() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_ICON_AGENT_CLAUDE.into(), "".into());
        opts.insert(tmux::SIDEBAR_ICON_AGENT_CODEX.into(), "".into());

        let icons = StatusIcons::from_options(&opts);
        assert_eq!(icons.agent_icon(&tmux::AgentType::Claude), "");
        assert_eq!(icons.agent_icon(&tmux::AgentType::Codex), "");
        // Untouched providers keep their defaults.
        assert_eq!(icons.agent_icon(&tmux::AgentType::OpenCode), "◇");
    }

    #[test]
    fn empty_agent_icon_option_falls_back_to_default() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_ICON_AGENT_CLAUDE.into(), "   ".into());
        let icons = StatusIcons::from_options(&opts);
        assert_eq!(icons.agent_icon(&tmux::AgentType::Claude), "✳");
    }
}
