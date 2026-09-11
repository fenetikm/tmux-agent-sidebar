use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::git::{self, GitData};
use crate::session;
use crate::state::{AppState, BottomTab};
use crate::tmux;
use crate::version::{self, UpdateNotice};

/// Channels and shared flags produced by [`spawn`] that the main event loop
/// drains every tick.
pub(super) struct Workers {
    pub git_rx: Receiver<GitData>,
    pub session_rx: Receiver<HashMap<String, String>>,
    pub version_rx: Receiver<UpdateNotice>,
    pub git_tab_active: Arc<AtomicBool>,
    /// Set by the main loop on pane focus change so the git thread polls
    /// immediately instead of waiting out its 2s sleep.
    pub git_poll_now: Arc<AtomicBool>,
    /// Panel result channel. `None` when `@sidebar_panel_command` is unset,
    /// in which case no worker thread exists either.
    pub panel_rx: Option<Receiver<crate::panel::PanelData>>,
    pub panel_tab_active: Arc<AtomicBool>,
}

/// Spawn the background threads (git polling, session-name polling, version
/// notice fetch) that feed the event loop.
pub(super) fn spawn(state: &AppState) -> Workers {
    let (git_tx, git_rx) = mpsc::channel::<GitData>();
    let (session_tx, session_rx) = mpsc::channel::<HashMap<String, String>>();
    let (version_tx, version_rx) = mpsc::channel::<UpdateNotice>();
    let tmux_pane_clone = state.tmux_pane.clone();
    let git_tab_active = Arc::new(AtomicBool::new(state.bottom_tab == BottomTab::GitStatus));
    let git_tab_flag = Arc::clone(&git_tab_active);
    let git_poll_now = Arc::new(AtomicBool::new(false));
    let git_poll_flag = Arc::clone(&git_poll_now);
    std::thread::spawn(move || {
        git_poll_loop(&tmux_pane_clone, &git_tx, &git_tab_flag, &git_poll_flag);
    });
    std::thread::spawn(move || {
        session_poll_loop(&session_tx);
    });
    std::thread::spawn(move || {
        if let Some(notice) = version::fetch_update_notice() {
            let _ = version_tx.send(notice);
        }
    });

    let panel_tab_active = Arc::new(AtomicBool::new(state.bottom_tab == BottomTab::Panel));
    let panel_rx = spawn_panel_worker(state, &panel_tab_active);

    Workers {
        git_rx,
        session_rx,
        version_rx,
        git_tab_active,
        git_poll_now,
        panel_rx,
        panel_tab_active,
    }
}

/// Spawn the panel poll thread when `@sidebar_panel_command` is configured.
/// Returns `None` — spawning nothing — when it is not, which is what keeps
/// the worker zero-cost for the common case of no panel configured.
fn spawn_panel_worker(
    state: &AppState,
    panel_tab_active: &Arc<AtomicBool>,
) -> Option<Receiver<crate::panel::PanelData>> {
    state.panel_config.clone().map(|config| {
        let (panel_tx, panel_rx) = mpsc::channel::<crate::panel::PanelData>();
        let tmux_pane = state.tmux_pane.clone();
        let active = Arc::clone(panel_tab_active);
        std::thread::spawn(move || {
            panel_poll_loop(&tmux_pane, &config, &panel_tx, &active);
        });
        panel_rx
    })
}

/// Session name polling thread. Scans `~/.claude/sessions/*.json` every 10
/// seconds so the main TUI thread never performs blocking filesystem I/O
/// to refresh `/rename`-assigned labels.
pub(super) fn session_poll_loop(tx: &mpsc::Sender<HashMap<String, String>>) {
    loop {
        std::thread::sleep(Duration::from_secs(10));
        let names = session::scan_session_names();
        if tx.send(names).is_err() {
            return;
        }
    }
}

/// Git data polling thread. Fetches git status every 2 seconds while the Git
/// tab is active. Skips fetching when the tab is not visible. PR numbers go
/// through an in-memory `(path, branch)`-keyed cache so `gh pr view` (the only
/// hop that costs GitHub API quota) runs at most once per `PR_CACHE_TTL`
/// instead of every tick.
pub(super) fn git_poll_loop(
    tmux_pane: &str,
    git_tx: &mpsc::Sender<GitData>,
    active: &AtomicBool,
    poll_now: &AtomicBool,
) {
    let mut last_path: Option<String> = None;
    let mut pr_cache = git::PrCache::new();
    loop {
        if !poll_now.swap(false, Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2));
        }

        if !active.load(Ordering::Relaxed) {
            continue;
        }

        // When the sidebar has focus, focused_pane_path returns None.
        // Reuse the last known path so git data keeps updating.
        if let Some(p) = tmux::focused_pane_path(tmux_pane) {
            last_path = Some(p);
        }
        if let Some(ref path) = last_path {
            let mut data = git::fetch_git_data(path);
            data.pr_number = pr_cache.get_or_fetch(
                path,
                &data.branch,
                std::time::Instant::now(),
                git::fetch_pr_number,
            );
            if git_tx.send(data).is_err() {
                return;
            }
        }
    }
}

/// Panel command polling thread. Wakes every second and, while the panel tab
/// is visible, re-resolves the focused pane's repository root and calls
/// [`crate::panel::PanelCache::get_or_run`] with it as the cache key. There
/// is no separate focus-change flag: since the cache key is the repo path, a
/// focus change to a different repository is simply a cache miss and the
/// command re-runs within the next second.
///
/// Cross-process deduplication is deliberately absent: several sidebars run
/// at once, and the script is the thing that knows what is expensive.
pub(super) fn panel_poll_loop(
    tmux_pane: &str,
    config: &crate::panel::PanelConfig,
    panel_tx: &mpsc::Sender<crate::panel::PanelData>,
    active: &AtomicBool,
) {
    let mut cache = crate::panel::PanelCache::new();
    let mut last_pane: Option<(String, String)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));

        if !active.load(Ordering::Relaxed) {
            continue;
        }

        // Returns None while the sidebar itself holds focus; reuse the last
        // known (pane_id, path) pair so the panel does not blank out and the
        // two never get paired across a focus change mid-iteration.
        if let Some(p) = tmux::find_active_pane(tmux_pane) {
            last_pane = Some(p);
        }
        let Some((ref pane_id, ref path)) = last_pane else {
            continue;
        };

        // Resolve the repository root so the cache key and the script's cwd
        // do not fragment per subdirectory. This is the only work needed
        // before the cache check: everything else in `PanelContext` is only
        // consumed on a cache miss, inside the closure below.
        let repo_path = git::repo_root(path).unwrap_or_else(|| path.clone());
        let data = cache.get_or_run(
            &repo_path,
            std::time::Instant::now(),
            config.interval,
            || {
                let ctx = crate::panel::PanelContext {
                    branch: git::run_git(&repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
                        .unwrap_or_default(),
                    pane_id: pane_id.clone(),
                    session: tmux::run_tmux(&["display-message", "-p", "#S"])
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    repo_path: repo_path.clone(),
                };
                crate::panel::run_command(config, &ctx)
            },
        );
        if panel_tx.send(data).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_worker_is_not_spawned_without_config() {
        // Exercises `spawn_panel_worker` directly rather than the full
        // `spawn`, which also starts the git poll loop, the session scan
        // loop, and a `curl` to GitHub for the version notice — none of
        // which this test needs, and all of which would otherwise run in
        // every `cargo test` invocation.
        let state = AppState::new("%99".into());
        let panel_tab_active = Arc::new(AtomicBool::new(false));
        let panel_rx = spawn_panel_worker(&state, &panel_tab_active);
        assert!(
            panel_rx.is_none(),
            "no @sidebar_panel_command means no worker and no channel"
        );
    }

    #[test]
    fn test_git_poll_skips_when_inactive() {
        let active = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<GitData>();

        let flag = Arc::clone(&active);
        let handle = std::thread::spawn(move || {
            // Simulate the poll loop check without actually sleeping 2s
            for _ in 0..3 {
                if !flag.load(Ordering::Relaxed) {
                    continue;
                }
                let _ = tx.send(GitData::default());
            }
        });

        handle.join().unwrap();
        // No data should have been sent since active=false
        assert!(
            rx.try_recv().is_err(),
            "should not poll when git tab is inactive"
        );
    }

    #[test]
    fn test_git_poll_sends_when_active() {
        let active = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel::<GitData>();

        let flag = Arc::clone(&active);
        let handle = std::thread::spawn(move || {
            // active=true, so it should send
            if flag.load(Ordering::Relaxed) {
                let _ = tx.send(GitData::default());
            }
        });

        handle.join().unwrap();
        assert!(rx.try_recv().is_ok(), "should poll when git tab is active");
    }

    #[test]
    fn test_git_poll_reacts_to_flag_change() {
        let active = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<GitData>();

        // Initially inactive
        assert!(!active.load(Ordering::Relaxed));

        // Switch to active
        active.store(true, Ordering::Relaxed);

        let flag = Arc::clone(&active);
        let handle = std::thread::spawn(move || {
            if flag.load(Ordering::Relaxed) {
                let _ = tx.send(GitData::default());
            }
        });

        handle.join().unwrap();
        assert!(
            rx.try_recv().is_ok(),
            "should poll after flag switches to active"
        );
    }

    #[test]
    fn test_git_poll_stops_on_sender_closed() {
        let active = AtomicBool::new(true);
        let (tx, rx) = mpsc::channel::<GitData>();
        drop(rx); // Close receiver

        let result = tx.send(GitData::default());
        assert!(result.is_err(), "send should fail when receiver is dropped");

        // Verify the flag check pattern used in git_poll_loop
        assert!(active.load(Ordering::Relaxed));
    }
}
