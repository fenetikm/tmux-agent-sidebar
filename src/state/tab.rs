use super::{AppState, BottomTab};

impl AppState {
    /// Auto-switch bottom tab based on the focused pane.
    ///
    /// - Focus changed → save old pane's tab, restore new pane's tab
    /// - New agent pane (first seen) → Activity tab (once only)
    /// - Non-agent pane with no saved pref → Git tab
    pub(crate) fn auto_switch_tab(&mut self) {
        let focus_changed =
            self.focus_state.focused_pane_id != self.focus_state.prev_focused_pane_id;
        if focus_changed {
            self.save_current_tab();
        }
        // detect_new_agents cleans up disappeared agents (removing their
        // seen status and saved tab prefs), so it must run after save
        // to avoid re-saving a stale pref for a closed agent.
        let new_agent_ids = self.detect_new_agents();

        match self.resolve_bottom_tab(focus_changed, &new_agent_ids) {
            TabDecision::Keep => {}
            TabDecision::Set(tab) => {
                self.bottom_tab = tab;
            }
        }

        if focus_changed {
            self.focus_state.prev_focused_pane_id = self.focus_state.focused_pane_id.clone();
        }
    }

    /// Decide what the bottom tab should do for the current focus state.
    ///
    /// `Keep` means "leave the current tab as-is".
    /// `Set(tab)` means "switch to the given tab now".
    fn resolve_bottom_tab(
        &self,
        focus_changed: bool,
        new_agent_pane_ids: &std::collections::HashSet<String>,
    ) -> TabDecision {
        if focus_changed {
            return self.resolve_tab_for_focused_pane(new_agent_pane_ids);
        }

        if let Some(ref fid) = self.focus_state.focused_pane_id
            && new_agent_pane_ids.contains(fid)
        {
            // Agent started in the currently focused pane.
            if self.show_activity_tab {
                return TabDecision::Set(BottomTab::Activity);
            }
        }

        TabDecision::Keep
    }

    /// Register all current agent panes. Returns IDs of newly appeared agents.
    /// Also removes agents that have disappeared so that re-launching
    /// an agent in the same pane is detected as new.
    fn detect_new_agents(&mut self) -> std::collections::HashSet<String> {
        let mut current: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut new_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for group in &self.repo_groups {
            for (pane, _) in &group.panes {
                current.insert(pane.pane_id.clone());
                if self.pane_states.seen.insert(pane.pane_id.clone()) {
                    new_ids.insert(pane.pane_id.clone());
                }
            }
        }
        // Remove disappeared agents from seen set so that relaunching an
        // agent is detected as new. Also clear the matching `tab_pref` so a
        // relaunched pane starts on the default tab — explicit here because
        // detect_new_agents can run before `prune_pane_states_to_current_panes`
        // (e.g. when tests mutate `repo_groups` directly without going
        // through `apply_session_snapshot`).
        let removed: Vec<String> = self
            .pane_states
            .seen
            .iter()
            .filter(|id| !current.contains(id.as_str()))
            .cloned()
            .collect();
        for id in &removed {
            self.pane_states.seen.remove(id);
            if let Some(state) = self.pane_states.get_mut(id) {
                state.tab_pref = None;
            }
        }
        new_ids
    }

    /// Save the current tab preference for the pane we're leaving.
    fn save_current_tab(&mut self) {
        if let Some(prev_id) = self.focus_state.prev_focused_pane_id.clone() {
            let tab = self.bottom_tab.clone();
            self.pane_state_mut(&prev_id).tab_pref = Some(tab);
        }
    }

    /// Restore the saved tab for the pane we're entering,
    /// or pick a sensible default.
    fn resolve_tab_for_focused_pane(
        &self,
        new_agent_pane_ids: &std::collections::HashSet<String>,
    ) -> TabDecision {
        let Some(ref cur_id) = self.focus_state.focused_pane_id else {
            return TabDecision::Keep;
        };
        let enabled = self.enabled_bottom_tabs();
        // A pane can hold a preference for a tab that has since been turned
        // off; restoring it would select a tab that no longer renders.
        if let Some(saved) = self.pane_state(cur_id).and_then(|s| s.tab_pref.as_ref())
            && enabled.contains(saved)
        {
            return TabDecision::Set(saved.clone());
        }
        let preferred = if new_agent_pane_ids.contains(cur_id) || self.focused_pane_is_agent() {
            // The focused pane is an agent, and there's no usable preference.
            BottomTab::Activity
        } else {
            BottomTab::GitStatus
        };
        if enabled.contains(&preferred) {
            TabDecision::Set(preferred)
        } else if let Some(first) = enabled.first() {
            TabDecision::Set(first.clone())
        } else {
            TabDecision::Keep
        }
    }

    /// Check if the focused pane is an agent pane (present in repo_groups).
    pub(crate) fn focused_pane_is_agent(&self) -> bool {
        let Some(ref fid) = self.focus_state.focused_pane_id else {
            return false;
        };
        self.repo_groups
            .iter()
            .any(|g| g.panes.iter().any(|(p, _)| p.pane_id == *fid))
    }

    /// The bottom tabs that currently exist, in display order.
    ///
    /// Every other tab decision — the title bar, the cycle order, which tab a
    /// pane falls back to — reads this, so "does this tab exist?" is answered
    /// in exactly one place. Empty when the user has turned all three off,
    /// which the renderer treats as `@sidebar_bottom_height 0`.
    pub fn enabled_bottom_tabs(&self) -> Vec<BottomTab> {
        let mut tabs = Vec::with_capacity(3);
        if self.show_activity_tab {
            tabs.push(BottomTab::Activity);
        }
        if self.show_git_tab {
            tabs.push(BottomTab::GitStatus);
        }
        if self.panel_enabled() {
            tabs.push(BottomTab::Panel);
        }
        tabs
    }

    /// Move off the current tab if it no longer exists. The toggles are
    /// re-read on every layout sync, so the tab under the cursor can be
    /// switched off from under it; without this the panel would render
    /// content whose title is no longer in the tab bar.
    pub fn clamp_bottom_tab(&mut self) {
        let enabled = self.enabled_bottom_tabs();
        if enabled.is_empty() || enabled.contains(&self.bottom_tab) {
            return;
        }
        self.bottom_tab = enabled[0].clone();
    }

    pub fn next_bottom_tab(&mut self) {
        let enabled = self.enabled_bottom_tabs();
        let Some(current) = enabled.iter().position(|t| *t == self.bottom_tab) else {
            // Current tab is gone; the clamp will have moved us already, but
            // a cycle before the next sync should still land somewhere real.
            if let Some(first) = enabled.first() {
                self.bottom_tab = first.clone();
            }
            return;
        };
        self.bottom_tab = enabled[(current + 1) % enabled.len()].clone();
    }

    /// Handle a mouse click on the bottom panel's tab header.
    ///
    /// Ranges come from `layout.bottom_tab_targets`, rebuilt each frame,
    /// because the panel tab's title is user-supplied and its columns
    /// cannot be hardcoded.
    pub fn handle_bottom_tab_click(&mut self, col: u16) {
        if let Some(target) = self
            .layout
            .bottom_tab_targets
            .iter()
            .find(|t| col >= t.start && col < t.end)
        {
            self.bottom_tab = target.tab.clone();
        }
    }

    pub fn scroll_bottom(&mut self, delta: isize) {
        match self.bottom_tab {
            BottomTab::Activity => self.activity.scroll.scroll(delta),
            BottomTab::GitStatus => self.scrolls.git.scroll(delta),
            BottomTab::Panel => self.scrolls.panel.scroll(delta),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TabDecision {
    Keep,
    Set(BottomTab),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::tmux::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorktreeMetadata};

    fn test_pane(id: &str) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: true,
            status: PaneStatus::Running,
            attention: false,
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
            tmux_session: String::new(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    fn agent_group(pane_id: &str) -> RepoGroup {
        RepoGroup {
            name: "project".into(),
            has_focus: true,
            session: None,
            panes: vec![(test_pane(pane_id), PaneGitInfo::default())],
        }
    }

    fn state_with_groups(repo_groups: Vec<RepoGroup>, focused_pane_id: Option<&str>) -> AppState {
        let mut state = AppState::new("%99".into());
        state.repo_groups = repo_groups;
        state.focus_state.focused_pane_id = focused_pane_id.map(str::to_string);
        state
    }

    // ─── focused_pane_is_agent ───────────────────────────────────

    #[test]
    fn focused_pane_is_agent_true() {
        let mut state = AppState::new("%99".into());
        state.focus_state.focused_pane_id = Some("%1".into());
        state.repo_groups = vec![agent_group("%1")];
        assert!(state.focused_pane_is_agent());
    }

    #[test]
    fn focused_pane_is_agent_false_non_agent() {
        let mut state = AppState::new("%99".into());
        state.focus_state.focused_pane_id = Some("%5".into());
        state.repo_groups = vec![agent_group("%1")];
        assert!(!state.focused_pane_is_agent());
    }

    #[test]
    fn focused_pane_is_agent_false_no_focus() {
        let mut state = AppState::new("%99".into());
        state.focus_state.focused_pane_id = None;
        state.repo_groups = vec![agent_group("%1")];
        assert!(!state.focused_pane_is_agent());
    }

    #[test]
    fn focused_pane_is_agent_false_empty_groups() {
        let mut state = AppState::new("%99".into());
        state.focus_state.focused_pane_id = Some("%1".into());
        state.repo_groups = vec![];
        assert!(!state.focused_pane_is_agent());
    }

    // ─── detect_new_agents ──────────────────────────────────────

    #[test]
    fn detect_new_agents_empty() {
        let mut state = AppState::new("%99".into());
        state.repo_groups = vec![];
        assert!(state.detect_new_agents().is_empty());
    }

    #[test]
    fn detect_new_agents_first_time() {
        let mut state = AppState::new("%99".into());
        state.repo_groups = vec![agent_group("%1")];
        let new_ids = state.detect_new_agents();
        assert!(new_ids.contains("%1"));
        assert!(state.pane_states.seen.contains("%1"));
    }

    #[test]
    fn detect_new_agents_already_seen() {
        let mut state = AppState::new("%99".into());
        state.pane_states.seen.insert("%1".into());
        state.repo_groups = vec![agent_group("%1")];
        assert!(state.detect_new_agents().is_empty());
    }

    // ─── scenario: full lifecycle ───────────────────────────────

    #[test]
    fn scenario_full_lifecycle() {
        let mut state = state_with_groups(vec![], Some("%5"));

        // Step 1: Sidebar starts, focus on non-agent pane %5
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "step 1: non-agent → Git"
        );

        // Step 2: Agent %1 starts, focus moves to it
        state.repo_groups = vec![agent_group("%1")];
        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "step 2: new agent → Activity"
        );

        // Step 3: Subsequent refresh (no focus change) → no change
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "step 3: same focus → no change"
        );

        // Step 4: User manually switches to Git
        state.next_bottom_tab();
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "step 4: manual Git → respected"
        );

        // Step 5: Focus to non-agent %5
        state.focus_state.focused_pane_id = Some("%5".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "step 5: non-agent → Git"
        );

        // Step 6: Focus back to %1 → restores saved Git pref
        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "step 6: restore %1's Git pref"
        );
    }

    // ─── scenario: per-pane tab memory ──────────────────────────

    #[test]
    fn scenario_per_pane_tab_memory() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        // Agent %1 → Activity
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        // User switches %1 to Git
        state.next_bottom_tab();

        // Agent %2 starts, focus moves to %2
        let mut group = agent_group("%1");
        group.panes.push((test_pane("%2"), PaneGitInfo::default()));
        state.repo_groups = vec![group];
        state.focus_state.focused_pane_id = Some("%2".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "%2: new agent → Activity"
        );

        // Focus back to %1 → Git (saved)
        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus, "%1: restored Git");

        // Focus back to %2 → Activity (saved)
        state.focus_state.focused_pane_id = Some("%2".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "%2: restored Activity"
        );
    }

    // ─── scenario: manual tab preserved across refreshes ────────

    #[test]
    fn scenario_manual_tab_preserved_across_refreshes() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        state.auto_switch_tab();

        // User switches to Git
        state.next_bottom_tab();

        // 5 refreshes (no focus change)
        for _ in 0..5 {
            state.auto_switch_tab();
        }
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "manual Git survives refreshes"
        );
    }

    // ─── scenario: new agent in same pane ───────────────────────

    #[test]
    fn scenario_new_agent_in_same_pane() {
        let mut state = state_with_groups(vec![], Some("%1"));

        // Focus on %1, no agent
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        // Agent starts in %1 (no focus change)
        state.repo_groups = vec![agent_group("%1")];
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "new agent without focus change"
        );
    }

    #[test]
    fn scenario_new_agent_elsewhere_does_not_override_existing_tab() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%5"));
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        // New agent appears in a different pane, but focus stays on the shell.
        let mut group = agent_group("%1");
        group.panes.push((test_pane("%2"), PaneGitInfo::default()));
        state.repo_groups = vec![group];
        state.auto_switch_tab();

        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "new agent elsewhere should not force a tab change"
        );
    }

    // ─── scenario: focus to existing agent (no saved pref) ──────

    #[test]
    fn scenario_focus_to_existing_agent_defaults_activity() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%5"));

        // %1 agent already seen, currently on non-agent %5
        state.pane_states.seen.insert("%1".into());
        state.focus_state.prev_focused_pane_id = Some("%5".into());
        state.bottom_tab = BottomTab::GitStatus;

        // Focus to %1 (no saved pref for %1)
        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "existing agent with no saved pref → Activity"
        );
    }

    // ─── scenario: focus changes to None ────────────────────────

    #[test]
    fn scenario_focus_becomes_none() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        // Focus becomes None (all panes closed?)
        state.focus_state.focused_pane_id = None;
        state.auto_switch_tab();
        // restore_or_default_tab returns early for None, so tab stays
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "None focus → tab unchanged"
        );
    }

    #[test]
    fn scenario_focus_none_then_returns_to_same_pane_preserves_tab() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        state.focus_state.focused_pane_id = None;
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "returning to the same pane after None should preserve the tab"
        );
    }

    #[test]
    fn scenario_restore_saved_tab_when_returning_to_pane() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        state.focus_state.focused_pane_id = Some("%5".into());
        state.repo_groups = vec![agent_group("%1")];
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        state.focus_state.focused_pane_id = Some("%1".into());
        state.repo_groups = vec![agent_group("%1")];
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "saved tab should be restored when focus returns"
        );
    }

    // ─── scenario: non-agent pane with other agent present ─────

    #[test]
    fn scenario_startup_non_agent_focus_with_other_agent() {
        // Sidebar starts, focus is on a non-agent pane but another
        // pane has an agent. The focused pane should get Git, not Activity.
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%5"));
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "non-agent pane should get Git even when other agents exist"
        );
    }

    #[test]
    fn scenario_focus_to_non_agent_while_agent_starts_elsewhere() {
        let mut state = state_with_groups(vec![], Some("%5"));

        // Start on %5 (shell)
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        // Agent %1 starts in another pane, but focus stays on %5
        state.repo_groups = vec![agent_group("%1")];
        // no focus change
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "should stay on Git when new agent is in a different pane"
        );
    }

    // ─── scenario: agent disappears then reappears ──────────────

    #[test]
    fn scenario_agent_closes_and_relaunches() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));

        // Agent %1 starts
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);

        // User switches to Git
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        // Agent %1 closes (disappears from repo_groups)
        state.repo_groups = vec![];
        state.focus_state.focused_pane_id = Some("%5".into());
        state.auto_switch_tab();
        // %1 should be removed from seen_agent_panes
        assert!(
            !state.pane_states.seen.contains("%1"),
            "closed agent should be removed from seen set"
        );

        // Agent relaunches in same pane %1
        state.repo_groups = vec![agent_group("%1")];
        state.focus_state.focused_pane_id = Some("%1".into());
        state.auto_switch_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::Activity,
            "relaunched agent should trigger Activity"
        );
    }

    // ─── scenario: panel tab ─────────────────────────────────────

    fn panel_config() -> crate::panel::PanelConfig {
        crate::panel::PanelConfig {
            command: "echo hi".into(),
            name: "PRs".into(),
            interval: std::time::Duration::from_secs(120),
            timeout: std::time::Duration::from_secs(10),
        }
    }

    #[test]
    fn tab_cycle_skips_panel_when_unconfigured() {
        let mut state = AppState::new("%99".into());
        state.bottom_tab = BottomTab::Activity;
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity, "no third tab exists");
    }

    #[test]
    fn tab_cycle_includes_panel_when_configured() {
        let mut state = AppState::new("%99".into());
        state.panel_config = Some(panel_config());
        state.bottom_tab = BottomTab::Activity;
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Panel);
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);
    }

    #[test]
    fn saved_panel_pref_falls_back_when_unconfigured() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));
        state.pane_state_mut("%1").tab_pref = Some(BottomTab::Panel);
        state.focus_state.prev_focused_pane_id = Some("%5".into());
        state.auto_switch_tab();
        // A preference for a tab that no longer exists is not restored; the
        // pane falls back to the tab it would have got with no preference at
        // all, which for an agent pane is Activity.
        assert_eq!(state.bottom_tab, BottomTab::Activity);
    }

    #[test]
    fn saved_panel_pref_is_restored_when_configured() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));
        state.panel_config = Some(panel_config());
        state.pane_state_mut("%1").tab_pref = Some(BottomTab::Panel);
        state.focus_state.prev_focused_pane_id = Some("%5".into());
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Panel);
    }

    #[test]
    fn scrolling_the_panel_tab_moves_the_panel_scroll() {
        let mut state = AppState::new("%99".into());
        state.panel_config = Some(panel_config());
        state.bottom_tab = BottomTab::Panel;
        state.scrolls.panel.total_lines = 50;
        state.scrolls.panel.visible_height = 10;
        state.scroll_bottom(3);
        assert_eq!(state.scrolls.panel.offset, 3);
        assert_eq!(state.scrolls.git.offset, 0, "git scroll must be untouched");
    }

    // ─── scenario: disabled Activity / Git tabs ──────────────────

    #[test]
    fn enabled_bottom_tabs_defaults_to_activity_and_git() {
        let state = AppState::new("%99".into());
        assert_eq!(
            state.enabled_bottom_tabs(),
            vec![BottomTab::Activity, BottomTab::GitStatus]
        );
    }

    #[test]
    fn enabled_bottom_tabs_includes_panel_when_configured() {
        let mut state = AppState::new("%99".into());
        state.panel_config = Some(panel_config());
        assert_eq!(
            state.enabled_bottom_tabs(),
            vec![BottomTab::Activity, BottomTab::GitStatus, BottomTab::Panel]
        );
    }

    #[test]
    fn enabled_bottom_tabs_omits_the_disabled_ones() {
        let mut state = AppState::new("%99".into());
        state.show_activity_tab = false;
        state.panel_config = Some(panel_config());
        assert_eq!(
            state.enabled_bottom_tabs(),
            vec![BottomTab::GitStatus, BottomTab::Panel]
        );
    }

    #[test]
    fn enabled_bottom_tabs_is_empty_when_everything_is_off() {
        let mut state = AppState::new("%99".into());
        state.show_activity_tab = false;
        state.show_git_tab = false;
        assert!(state.enabled_bottom_tabs().is_empty());
    }

    #[test]
    fn tab_cycle_skips_a_disabled_tab() {
        let mut state = AppState::new("%99".into());
        state.show_activity_tab = false;
        state.panel_config = Some(panel_config());
        state.bottom_tab = BottomTab::GitStatus;
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Panel);
        state.next_bottom_tab();
        assert_eq!(
            state.bottom_tab,
            BottomTab::GitStatus,
            "Activity is skipped"
        );
    }

    #[test]
    fn tab_cycle_is_a_noop_with_one_enabled_tab() {
        let mut state = AppState::new("%99".into());
        state.show_activity_tab = false;
        state.bottom_tab = BottomTab::GitStatus;
        state.next_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
    }

    #[test]
    fn saved_pref_for_a_disabled_tab_falls_back() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));
        state.show_activity_tab = false;
        state.pane_state_mut("%1").tab_pref = Some(BottomTab::Activity);
        state.focus_state.prev_focused_pane_id = Some("%5".into());
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
    }

    #[test]
    fn agent_pane_falls_back_to_git_when_activity_is_off() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%1"));
        state.show_activity_tab = false;
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
    }

    #[test]
    fn non_agent_pane_falls_back_to_activity_when_git_is_off() {
        let mut state = state_with_groups(vec![agent_group("%1")], Some("%5"));
        state.show_git_tab = false;
        state.auto_switch_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);
    }

    #[test]
    fn clamp_moves_off_a_tab_that_was_just_disabled() {
        let mut state = AppState::new("%99".into());
        state.bottom_tab = BottomTab::GitStatus;
        state.show_git_tab = false;
        state.clamp_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::Activity);
    }

    #[test]
    fn clamp_leaves_an_enabled_tab_alone() {
        let mut state = AppState::new("%99".into());
        state.bottom_tab = BottomTab::GitStatus;
        state.clamp_bottom_tab();
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);
    }
}
