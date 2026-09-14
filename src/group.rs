use std::collections::HashMap;
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::state::{RepoFilter, StatusFilter};
use crate::tmux::PaneInfo;

/// Per-pane git metadata resolved from the pane's working directory.
#[derive(Debug, Clone, Default)]
pub struct PaneGitInfo {
    pub repo_root: Option<String>,
    pub branch: Option<String>,
    pub is_worktree: bool,
    pub worktree_name: Option<String>,
}

/// How the agent list is grouped, from the `@sidebar_sorting` tmux option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// One group per repository, spanning every tmux session. The default.
    #[default]
    Repository,
    /// One group per (tmux session, repository) pair, so each session's
    /// repos sit together under a session header.
    Session,
}

/// A group of panes working in the same repository (or directory).
#[derive(Debug, Clone)]
pub struct RepoGroup {
    /// Display name: repo directory basename, or raw path for non-git
    pub name: String,
    /// The tmux session this group belongs to, in `SortMode::Session`.
    /// `None` in `SortMode::Repository`, where groups span sessions.
    /// `None` (or empty) is what tells the renderer to emit no session
    /// header, so both modes share one rendering path.
    pub session: Option<String>,
    /// Whether any pane in the group belongs to the focused (active) window
    pub has_focus: bool,
    /// Panes in this group, with their git info
    pub panes: Vec<(PaneInfo, PaneGitInfo)>,
}

/// How long a pane path's git info is trusted before it is re-read from
/// disk. A branch switch shows up in the sidebar within this window.
///
/// This exists because every sidebar instance calls [`group_panes`] once a
/// second, and each call used to run one or two `git` processes per agent
/// pane. With a sidebar per window that is `sidebars × panes` forks per
/// second — measured at ~245/s on a busy day, which after six hours wedged
/// launchd and opendirectoryd badly enough to need a hard reboot. The
/// resolver no longer forks at all ([`resolve_pane_git_info`] reads the
/// `.git` layout directly); the cache now just bounds filesystem reads.
pub const GIT_INFO_TTL: Duration = Duration::from_secs(30);

/// Per-path cache of [`PaneGitInfo`] that survives across refresh ticks.
/// Negative results (non-git directories) are cached too: they are the
/// expensive case, walking every ancestor up to `/` per miss.
#[derive(Debug, Default)]
pub struct GitInfoCache {
    entries: HashMap<String, GitInfoEntry>,
}

#[derive(Debug)]
struct GitInfoEntry {
    info: PaneGitInfo,
    resolved_at: Instant,
}

impl GitInfoCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached info for `path` when it is younger than
    /// [`GIT_INFO_TTL`], otherwise call `resolver`, store, and return.
    pub fn get_or_resolve<F>(&mut self, path: &str, now: Instant, resolver: F) -> PaneGitInfo
    where
        F: FnOnce(&str) -> PaneGitInfo,
    {
        if let Some(entry) = self.entries.get(path)
            && now.duration_since(entry.resolved_at) < GIT_INFO_TTL
        {
            return entry.info.clone();
        }
        let info = resolver(path);
        self.entries.insert(
            path.to_string(),
            GitInfoEntry {
                info: info.clone(),
                resolved_at: now,
            },
        );
        info
    }

    /// Drop every entry whose path is not in `paths`, so the cache tracks
    /// the panes that currently exist rather than growing forever.
    pub fn retain_only(&mut self, paths: &[String]) {
        self.entries.retain(|path, _| paths.contains(path));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// When `path` was last resolved, if it is cached. Test seam.
    #[cfg(test)]
    pub fn resolved_at(&self, path: &str) -> Option<Instant> {
        self.entries.get(path).map(|e| e.resolved_at)
    }
}

/// Resolve git info for a single pane path by reading the repository's
/// on-disk layout directly — no `git` process is spawned.
///
/// Every sidebar calls this (via [`group_panes_with_cache`]) for every
/// agent pane path, so a fork here multiplies by `sidebars × panes`. The
/// files involved are stable, documented git internals: `.git` (directory,
/// or a `gitdir:` pointer file for linked worktrees and submodules), the
/// git dir's `HEAD`, and — only in a linked worktree — its `commondir`
/// back-reference to the main repository's `.git`.
pub fn resolve_pane_git_info(path: &str) -> PaneGitInfo {
    if path.is_empty() {
        return PaneGitInfo::default();
    }

    let Some((work_root, git_dir)) = discover_git_dir(std::path::Path::new(path)) else {
        return PaneGitInfo::default();
    };

    let branch = read_head_branch(&git_dir);

    // Only a linked worktree's git dir carries a `commondir` file; the
    // main checkout's `.git` never does. That is the same rule git uses
    // for `--git-common-dir`, so `is_worktree` matches what `git rev-parse`
    // used to report. Worktrees group under the main checkout, whose root
    // is the parent of the common dir.
    let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|rel| resolve_git_path(&git_dir.to_string_lossy(), rel.trim()));
    let is_worktree = common_dir.is_some();
    let repo_root = match common_dir {
        Some(common) => common.parent().map(|p| p.to_string_lossy().into_owned()),
        None => Some(work_root.to_string_lossy().into_owned()),
    };

    PaneGitInfo {
        repo_root,
        branch,
        is_worktree,
        worktree_name: None,
    }
}

/// Walk up from `path` to the nearest directory holding a `.git` entry.
/// Returns `(work_root, git_dir)`, both canonicalised where possible. A
/// `.git` *file* is a `gitdir: <path>` pointer (linked worktree or
/// submodule) resolved relative to the directory that contains it.
fn discover_git_dir(path: &std::path::Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    for dir in path.ancestors() {
        let dot_git = dir.join(".git");
        let meta = match std::fs::symlink_metadata(&dot_git) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let work_root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if meta.is_dir() {
            let git_dir = dot_git.canonicalize().unwrap_or(dot_git);
            return Some((work_root, git_dir));
        }
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let target = pointer.trim().strip_prefix("gitdir:")?.trim();
        let git_dir = resolve_git_path(&dir.to_string_lossy(), target);
        return Some((work_root, git_dir));
    }
    None
}

/// Branch name from the git dir's `HEAD`, matching what
/// `git rev-parse --abbrev-ref HEAD` printed: the short branch name for a
/// symbolic ref, or the literal `HEAD` when detached. Unlike `rev-parse`,
/// an unborn branch (fresh `git init`, no commits) still yields its name.
fn read_head_branch(git_dir: &std::path::Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    Some(match head.strip_prefix("ref:") {
        Some(target) => target
            .trim()
            .strip_prefix("refs/heads/")
            .unwrap_or(target.trim())
            .to_string(),
        None => "HEAD".to_string(),
    })
}

/// Group all panes across all sessions.
///
/// In [`SortMode::Repository`] every pane of a repo lands in one group,
/// whatever session it sits in. In [`SortMode::Session`] the same repo
/// open in two sessions yields two groups, each listing only that
/// session's panes.
///
/// Groups are returned sorted by `(session, display name)`,
/// case-insensitively. In repository mode every session is `None`, so the
/// key degenerates to the display name and the order is unchanged. The
/// attached session is not pinned to the top: a block that moves when you
/// switch sessions is harder to build a spatial memory of.
///
/// One-shot form for CLI callers: resolves git info fresh. The TUI, which
/// calls this every second, must use [`group_panes_with_cache`] instead.
pub fn group_panes(sessions: &[crate::tmux::SessionInfo], mode: SortMode) -> Vec<RepoGroup> {
    group_panes_with_cache(sessions, mode, &mut GitInfoCache::new(), Instant::now())
}

/// [`group_panes`] with a caller-owned [`GitInfoCache`], so repeated calls
/// only re-resolve paths that are new or older than [`GIT_INFO_TTL`].
/// Entries for paths no longer present in `sessions` are evicted.
pub fn group_panes_with_cache(
    sessions: &[crate::tmux::SessionInfo],
    mode: SortMode,
    git_cache: &mut GitInfoCache,
    now: Instant,
) -> Vec<RepoGroup> {
    let mut groups: IndexMap<(Option<String>, String), RepoGroup> = IndexMap::new();
    let mut seen_paths: Vec<String> = Vec::new();

    for session in sessions {
        for window in &session.windows {
            for pane in &window.panes {
                if !seen_paths.contains(&pane.path) {
                    seen_paths.push(pane.path.clone());
                }
                let mut git_info = git_cache.get_or_resolve(&pane.path, now, resolve_pane_git_info);

                // Override with hook-provided worktree info (Claude Code
                // provides this; Codex does not, so the git-command base
                // remains as fallback).
                if !pane.worktree.name.is_empty() {
                    git_info.worktree_name = Some(pane.worktree.name.clone());
                    git_info.is_worktree = true;
                }
                if !pane.worktree.branch.is_empty() {
                    git_info.branch = Some(pane.worktree.branch.clone());
                    git_info.is_worktree = true;
                }

                let repo_key = match &git_info.repo_root {
                    Some(root) => root.clone(),
                    None => pane.path.clone(),
                };

                let display_name = repo_key.rsplit('/').next().unwrap_or(&repo_key).to_string();

                let session_key = match mode {
                    SortMode::Repository => None,
                    SortMode::Session => Some(pane.tmux_session.clone()),
                };

                let has_focus = window.window_active && pane.pane_active;

                let group = groups
                    .entry((session_key.clone(), repo_key))
                    .or_insert_with(|| RepoGroup {
                        name: display_name,
                        session: session_key,
                        has_focus: false,
                        panes: Vec::new(),
                    });

                if has_focus {
                    group.has_focus = true;
                }

                group.panes.push((pane.clone(), git_info));
            }
        }
    }

    git_cache.retain_only(&seen_paths);

    let mut result: Vec<RepoGroup> = groups.into_values().collect();
    result.sort_by_key(|group| {
        (
            group.session.as_deref().unwrap_or_default().to_lowercase(),
            group.name.to_lowercase(),
        )
    });
    result
}

/// Agent pane ids visible in the sidebar list: repo groups in order, status
/// and repo filters applied. Stale repo filters (name no longer present) fall
/// back to `All` without writing back to tmux — only the running TUI persists
/// that correction.
pub fn visible_pane_ids(
    groups: &[RepoGroup],
    status_filter: StatusFilter,
    repo_filter: &RepoFilter,
) -> Vec<String> {
    let effective_repo = match repo_filter {
        RepoFilter::All => RepoFilter::All,
        RepoFilter::Repo(name) if groups.iter().any(|g| g.name == *name) => {
            RepoFilter::Repo(name.clone())
        }
        RepoFilter::Repo(_) => RepoFilter::All,
    };

    let mut ids = Vec::new();
    for group in groups {
        if !effective_repo.matches_group(&group.name) {
            continue;
        }
        for (pane, _) in &group.panes {
            if status_filter.matches(&pane.status) {
                ids.push(pane.pane_id.clone());
            }
        }
    }
    ids
}

/// Resolve a possibly-relative git path to an absolute canonical path.
fn resolve_git_path(base: &str, git_path: &str) -> std::path::PathBuf {
    let p = if std::path::Path::new(git_path).is_absolute() {
        std::path::PathBuf::from(git_path)
    } else {
        std::path::PathBuf::from(base).join(git_path)
    };
    p.canonicalize().unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_git_info_returns_none_for_empty_path() {
        let info = resolve_pane_git_info("");
        assert!(info.branch.is_none());
        assert!(!info.is_worktree);
        assert!(info.repo_root.is_none());
    }

    #[test]
    fn resolve_git_info_for_real_repo() {
        // This test runs in the actual repo, so git commands work
        let info = resolve_pane_git_info(env!("CARGO_MANIFEST_DIR"));
        assert!(info.repo_root.is_some(), "should detect git repo");
        assert!(info.branch.is_some(), "should detect branch");
        let root = info.repo_root.unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let manifest_dir = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        assert_eq!(root, manifest_dir, "repo root should be manifest dir");
    }

    #[test]
    fn worktree_and_main_share_same_repo_root() {
        // Both main and worktree should resolve to the same repo_root
        // We can only test the main worktree here, but verify the logic is consistent
        let info = resolve_pane_git_info(env!("CARGO_MANIFEST_DIR"));
        assert!(
            !info.is_worktree,
            "main checkout should not be detected as worktree"
        );
        assert!(info.repo_root.is_some());
    }

    // ─── fork-free resolve_pane_git_info tests ──────────────────────
    //
    // Fixtures are built by hand — a `.git` directory holding only `HEAD`,
    // which `git` itself refuses to recognise as a repository — so these
    // pass only if the resolver reads the files directly and never spawns
    // `git`. That is the property that matters: `group_panes` runs this
    // once per pane path per sidebar, and spawning `git` there is what
    // took the machine down (see `GIT_INFO_TTL`).

    fn fake_repo(root: &std::path::Path, head: &str) {
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), head).unwrap();
    }

    /// Lay out a linked worktree at `wt` the way `git worktree add` does:
    /// `wt/.git` is a file pointing at `main/.git/worktrees/<name>`, which
    /// holds its own `HEAD` plus a `commondir` back-reference.
    fn fake_worktree(main: &std::path::Path, wt: &std::path::Path, name: &str, head: &str) {
        let gitdir = main.join(".git/worktrees").join(name);
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), head).unwrap();
        std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    }

    fn canon(p: &std::path::Path) -> String {
        std::fs::canonicalize(p)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn resolve_git_info_reads_branch_from_head_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fake_repo(&root, "ref: refs/heads/feat/x\n");

        let info = resolve_pane_git_info(root.to_str().unwrap());

        assert_eq!(info.branch.as_deref(), Some("feat/x"));
        assert_eq!(info.repo_root, Some(canon(&root)));
        assert!(!info.is_worktree);
    }

    #[test]
    fn resolve_git_info_walks_up_from_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fake_repo(&root, "ref: refs/heads/main\n");
        let deep = root.join("sub/deep");
        std::fs::create_dir_all(&deep).unwrap();

        let info = resolve_pane_git_info(deep.to_str().unwrap());

        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.repo_root, Some(canon(&root)));
        assert!(!info.is_worktree);
    }

    #[test]
    fn resolve_git_info_detached_head_reports_head_like_git() {
        // `git rev-parse --abbrev-ref HEAD` prints the literal `HEAD` when
        // detached; keep that so the row renders the same as before.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fake_repo(&root, "4ed29f40bfcb1d6421b115b46752b803f6b8797d\n");

        let info = resolve_pane_git_info(root.to_str().unwrap());

        assert_eq!(info.branch.as_deref(), Some("HEAD"));
        assert_eq!(info.repo_root, Some(canon(&root)));
    }

    #[test]
    fn resolve_git_info_worktree_shares_main_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let wt = tmp.path().join("wt");
        fake_repo(&main, "ref: refs/heads/main\n");
        fake_worktree(&main, &wt, "wt", "ref: refs/heads/wt-branch\n");

        let info = resolve_pane_git_info(wt.to_str().unwrap());

        assert!(info.is_worktree, "linked worktree must be flagged");
        assert_eq!(info.branch.as_deref(), Some("wt-branch"));
        assert_eq!(
            info.repo_root,
            Some(canon(&main)),
            "worktree groups under the main checkout"
        );
    }

    #[test]
    fn resolve_git_info_worktree_resolves_relative_gitdir() {
        // `.git` files written by some tools (and by git for submodules)
        // use a path relative to the directory holding the `.git` file.
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let wt = tmp.path().join("wt");
        fake_repo(&main, "ref: refs/heads/main\n");
        fake_worktree(&main, &wt, "wt", "ref: refs/heads/wt-branch\n");
        std::fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();

        let info = resolve_pane_git_info(wt.to_str().unwrap());

        assert!(info.is_worktree);
        assert_eq!(info.branch.as_deref(), Some("wt-branch"));
        assert_eq!(info.repo_root, Some(canon(&main)));
    }

    #[test]
    fn resolve_git_info_non_repo_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plain");
        std::fs::create_dir_all(&dir).unwrap();

        let info = resolve_pane_git_info(dir.to_str().unwrap());

        assert!(info.repo_root.is_none());
        assert!(info.branch.is_none());
        assert!(!info.is_worktree);
    }

    #[test]
    fn resolve_git_info_missing_path_returns_default() {
        // A pane whose cwd was deleted still reports a path; walking a
        // nonexistent ancestor chain must not panic or find a stray repo.
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("gone/away");

        let info = resolve_pane_git_info(gone.to_str().unwrap());

        assert!(info.repo_root.is_none());
        assert!(info.branch.is_none());
    }

    // ─── resolve_git_path tests ─────────────────────────────────────

    #[test]
    fn resolve_git_path_absolute() {
        let result = resolve_git_path("/base", "/absolute/path");
        assert_eq!(result, std::path::PathBuf::from("/absolute/path"));
    }

    #[test]
    fn resolve_git_path_relative() {
        let result = resolve_git_path("/base/dir", "relative");
        assert_eq!(result, std::path::PathBuf::from("/base/dir/relative"));
    }

    // ─── GitInfoCache tests ─────────────────────────────────────────

    fn counting_resolver(calls: &std::cell::Cell<u32>) -> impl Fn(&str) -> PaneGitInfo + '_ {
        move |path| {
            calls.set(calls.get() + 1);
            PaneGitInfo {
                repo_root: Some(path.to_string()),
                branch: Some("main".into()),
                is_worktree: false,
                worktree_name: None,
            }
        }
    }

    #[test]
    fn git_info_cache_resolves_a_path_once_within_ttl() {
        let calls = std::cell::Cell::new(0);
        let mut cache = GitInfoCache::new();
        let t0 = std::time::Instant::now();

        let first = cache.get_or_resolve("/repo", t0, counting_resolver(&calls));
        let second =
            cache.get_or_resolve("/repo", t0 + GIT_INFO_TTL / 2, counting_resolver(&calls));

        assert_eq!(
            calls.get(),
            1,
            "second lookup within TTL must not re-resolve"
        );
        assert_eq!(first.repo_root, second.repo_root);
        assert_eq!(first.branch, second.branch);
    }

    #[test]
    fn git_info_cache_re_resolves_after_ttl() {
        let calls = std::cell::Cell::new(0);
        let mut cache = GitInfoCache::new();
        let t0 = std::time::Instant::now();

        cache.get_or_resolve("/repo", t0, counting_resolver(&calls));
        cache.get_or_resolve("/repo", t0 + GIT_INFO_TTL, counting_resolver(&calls));

        assert_eq!(calls.get(), 2, "lookup at or past TTL must resolve again");
    }

    #[test]
    fn git_info_cache_caches_non_git_paths_too() {
        // A non-git directory is the expensive case (an ancestor walk to
        // `/`), so the negative result must be cached just like a positive one.
        let calls = std::cell::Cell::new(0);
        let mut cache = GitInfoCache::new();
        let t0 = std::time::Instant::now();
        let resolver = |_: &str| {
            calls.set(calls.get() + 1);
            PaneGitInfo::default()
        };

        cache.get_or_resolve("/not/a/repo", t0, resolver);
        let info = cache.get_or_resolve("/not/a/repo", t0 + GIT_INFO_TTL / 2, resolver);

        assert_eq!(calls.get(), 1);
        assert!(info.repo_root.is_none());
    }

    #[test]
    fn git_info_cache_retain_drops_paths_no_longer_present() {
        let calls = std::cell::Cell::new(0);
        let mut cache = GitInfoCache::new();
        let t0 = std::time::Instant::now();

        cache.get_or_resolve("/a", t0, counting_resolver(&calls));
        cache.get_or_resolve("/b", t0, counting_resolver(&calls));
        cache.retain_only(&["/a".to_string()]);

        assert_eq!(cache.len(), 1);
        cache.get_or_resolve("/b", t0, counting_resolver(&calls));
        assert_eq!(
            calls.get(),
            3,
            "/b was evicted so it must be resolved again"
        );
    }

    #[test]
    fn group_panes_with_cache_reuses_git_info_across_ticks() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let sessions = vec![test_session(vec![test_window(
            vec![test_pane("%1", manifest_dir)],
            true,
        )])];
        let mut cache = GitInfoCache::new();
        let t0 = std::time::Instant::now();

        group_panes_with_cache(&sessions, SortMode::Repository, &mut cache, t0);
        let resolved_at = cache.resolved_at(manifest_dir);
        group_panes_with_cache(
            &sessions,
            SortMode::Repository,
            &mut cache,
            t0 + GIT_INFO_TTL / 2,
        );

        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.resolved_at(manifest_dir),
            resolved_at,
            "second tick within TTL must reuse the cached entry"
        );
    }

    // ─── group_panes tests ──────────────────────────────────────────

    fn test_pane(id: &str, path: &str) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status: crate::tmux::PaneStatus::Running,
            attention: false,
            agent: crate::tmux::AgentType::Claude,
            path: path.into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: crate::tmux::PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: crate::tmux::WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            tmux_session: String::new(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    fn test_pane_in_session(id: &str, path: &str, tmux_session: &str) -> PaneInfo {
        let mut pane = test_pane(id, path);
        pane.tmux_session = tmux_session.into();
        pane
    }

    fn test_window(panes: Vec<PaneInfo>, active: bool) -> crate::tmux::WindowInfo {
        crate::tmux::WindowInfo {
            window_id: "@0".into(),
            window_name: "test".into(),
            window_active: active,
            auto_rename: false,
            panes,
        }
    }

    fn test_session(windows: Vec<crate::tmux::WindowInfo>) -> crate::tmux::SessionInfo {
        crate::tmux::SessionInfo {
            session_name: "main".into(),
            windows,
        }
    }

    #[test]
    fn group_panes_empty_sessions() {
        let groups = group_panes(&[], SortMode::Repository);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_panes_same_repo() {
        // Two panes in the same real repo should be grouped together
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pane1 = test_pane("%1", manifest_dir);
        let pane2 = test_pane("%2", manifest_dir);

        let sessions = vec![test_session(vec![test_window(vec![pane1, pane2], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(groups.len(), 1, "same repo path should produce one group");
        assert_eq!(groups[0].panes.len(), 2);
        assert_eq!(groups[0].panes[0].0.pane_id, "%1");
        assert_eq!(groups[0].panes[1].0.pane_id, "%2");
    }

    #[test]
    fn group_panes_non_git_path_uses_raw_path() {
        // A non-git path should use the raw path as the group key
        let pane = test_pane("%1", "/tmp/no-git-here");

        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "no-git-here");
    }

    #[test]
    fn group_panes_display_name_is_basename() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pane = test_pane("%1", manifest_dir);

        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(groups.len(), 1);
        let expected_name = std::path::Path::new(manifest_dir)
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert_eq!(
            groups[0].name, expected_name,
            "display name should be repo basename"
        );
    }

    #[test]
    fn group_panes_has_focus_from_active_window_and_pane() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let mut pane = test_pane("%1", manifest_dir);
        pane.pane_active = true;

        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert!(
            groups[0].has_focus,
            "active pane in active window should set has_focus"
        );
    }

    #[test]
    fn group_panes_no_focus_when_window_inactive() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let mut pane = test_pane("%1", manifest_dir);
        pane.pane_active = true;

        let sessions = vec![test_session(vec![test_window(vec![pane], false)])]; // window_active=false
        let groups = group_panes(&sessions, SortMode::Repository);

        assert!(
            !groups[0].has_focus,
            "active pane in inactive window should not set has_focus"
        );
    }

    #[test]
    fn group_panes_empty_path_pane() {
        let pane = test_pane("%1", "");

        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        // Empty path pane should still be grouped (by empty key)
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn group_panes_multiple_sessions() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pane1 = test_pane("%1", manifest_dir);
        let pane2 = test_pane("%2", "/tmp/other-project");

        let sessions = vec![
            test_session(vec![test_window(vec![pane1], true)]),
            crate::tmux::SessionInfo {
                session_name: "other".into(),
                windows: vec![test_window(vec![pane2], false)],
            },
        ];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(
            groups.len(),
            2,
            "different repos across sessions should produce separate groups"
        );
    }

    #[test]
    fn group_panes_same_repo_across_sessions_merge_into_one_group() {
        // Regression for the `state.sessions` field removal: panes that
        // live in different tmux sessions but share the same repo path
        // must still collapse into a single `RepoGroup`. This is what
        // makes the sidebar usable across multi-session workflows.
        // `SortMode::Session` inverts this guarantee deliberately: see
        // `group_panes_same_repo_across_sessions_splits_in_session_mode`
        // below.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pane_session_a = test_pane("%1", manifest_dir);
        let pane_session_b = test_pane("%2", manifest_dir);

        let sessions = vec![
            crate::tmux::SessionInfo {
                session_name: "alpha".into(),
                windows: vec![test_window(vec![pane_session_a], true)],
            },
            crate::tmux::SessionInfo {
                session_name: "beta".into(),
                windows: vec![test_window(vec![pane_session_b], false)],
            },
        ];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(
            groups.len(),
            1,
            "panes in the same repo across sessions must merge into one group"
        );
        assert_eq!(groups[0].panes.len(), 2);
        let pane_ids: Vec<&str> = groups[0]
            .panes
            .iter()
            .map(|(p, _)| p.pane_id.as_str())
            .collect();
        assert!(pane_ids.contains(&"%1"));
        assert!(pane_ids.contains(&"%2"));
    }

    #[test]
    fn group_panes_same_repo_across_sessions_splits_in_session_mode() {
        // The deliberate inversion of
        // `group_panes_same_repo_across_sessions_merge_into_one_group`:
        // in session mode a repo open in two sessions is two groups, each
        // listing only that session's panes.
        let pane_a = test_pane_in_session("%1", "/tmp/shared-repo", "alpha");
        let pane_b = test_pane_in_session("%2", "/tmp/shared-repo", "beta");

        let sessions = vec![
            crate::tmux::SessionInfo {
                session_name: "alpha".into(),
                windows: vec![test_window(vec![pane_a], true)],
            },
            crate::tmux::SessionInfo {
                session_name: "beta".into(),
                windows: vec![test_window(vec![pane_b], false)],
            },
        ];
        let groups = group_panes(&sessions, SortMode::Session);

        assert_eq!(groups.len(), 2, "one group per session");
        assert_eq!(groups[0].session.as_deref(), Some("alpha"));
        assert_eq!(groups[0].name, "shared-repo");
        assert_eq!(groups[0].panes.len(), 1);
        assert_eq!(groups[0].panes[0].0.pane_id, "%1");
        assert_eq!(groups[1].session.as_deref(), Some("beta"));
        assert_eq!(groups[1].panes[0].0.pane_id, "%2");
    }

    #[test]
    fn group_panes_repository_mode_leaves_session_unset() {
        let pane = test_pane_in_session("%1", "/tmp/some-repo", "alpha");
        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].session, None,
            "repository groups span sessions, so they carry no session"
        );
    }

    #[test]
    fn group_panes_sorts_by_session_then_name_case_insensitively() {
        let sessions = vec![test_session(vec![test_window(
            vec![
                test_pane_in_session("%1", "/tmp/zzz", "work"),
                test_pane_in_session("%2", "/tmp/aaa", "work"),
                test_pane_in_session("%3", "/tmp/mmm", "Personal"),
            ],
            true,
        )])];
        let groups = group_panes(&sessions, SortMode::Session);

        let order: Vec<(&str, &str)> = groups
            .iter()
            .map(|g| (g.session.as_deref().unwrap_or(""), g.name.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![("Personal", "mmm"), ("work", "aaa"), ("work", "zzz")]
        );
    }

    #[test]
    fn group_panes_blank_tmux_session_keeps_an_empty_session_key() {
        // `tmux_session` is blank across many fixtures and can be blank in
        // practice; grouping must not panic or drop the pane. The renderer
        // is what suppresses the header for a blank name.
        let pane = test_pane_in_session("%1", "/tmp/orphan", "");
        let sessions = vec![test_session(vec![test_window(vec![pane], true)])];
        let groups = group_panes(&sessions, SortMode::Session);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].session.as_deref(), Some(""));
        assert_eq!(groups[0].panes.len(), 1);
    }

    #[test]
    fn group_panes_sorted_by_name_case_insensitive() {
        // Groups should be sorted alphabetically regardless of encounter order
        let pane1 = test_pane("%1", "/tmp/zzz");
        let pane2 = test_pane("%2", "/tmp/Aaa");
        let pane3 = test_pane("%3", "/tmp/mmm");
        let pane4 = test_pane("%4", "/tmp/zzz");

        let sessions = vec![test_session(vec![test_window(
            vec![pane1, pane2, pane3, pane4],
            true,
        )])];
        let groups = group_panes(&sessions, SortMode::Repository);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].name, "Aaa");
        assert_eq!(groups[1].name, "mmm");
        assert_eq!(groups[2].name, "zzz");
        assert_eq!(groups[2].panes.len(), 2, "zzz should have 2 panes");
    }

    fn test_pane_with_status(id: &str, status: crate::tmux::PaneStatus) -> PaneInfo {
        let mut pane = test_pane(id, "/repo");
        pane.status = status;
        pane
    }

    fn test_group_with_status(
        name: &str,
        pane_ids: &[(&str, crate::tmux::PaneStatus)],
    ) -> RepoGroup {
        RepoGroup {
            name: name.into(),
            session: None,
            has_focus: false,
            panes: pane_ids
                .iter()
                .map(|(id, status)| {
                    (
                        test_pane_with_status(id, status.clone()),
                        PaneGitInfo::default(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn visible_pane_ids_returns_all_agents_when_unfiltered() {
        let groups = vec![
            test_group_with_status("alpha", &[("%1", crate::tmux::PaneStatus::Running)]),
            test_group_with_status("beta", &[("%2", crate::tmux::PaneStatus::Idle)]),
        ];
        assert_eq!(
            visible_pane_ids(&groups, StatusFilter::All, &RepoFilter::All),
            vec!["%1", "%2"]
        );
    }

    #[test]
    fn visible_pane_ids_applies_status_filter() {
        let groups = vec![test_group_with_status(
            "app",
            &[
                ("%1", crate::tmux::PaneStatus::Running),
                ("%2", crate::tmux::PaneStatus::Idle),
                ("%3", crate::tmux::PaneStatus::Waiting),
            ],
        )];
        assert_eq!(
            visible_pane_ids(&groups, StatusFilter::Idle, &RepoFilter::All),
            vec!["%2"]
        );
    }

    #[test]
    fn visible_pane_ids_applies_repo_filter() {
        let groups = vec![
            test_group_with_status("app", &[("%1", crate::tmux::PaneStatus::Running)]),
            test_group_with_status("lib", &[("%2", crate::tmux::PaneStatus::Running)]),
        ];
        assert_eq!(
            visible_pane_ids(&groups, StatusFilter::All, &RepoFilter::Repo("lib".into())),
            vec!["%2"]
        );
    }

    #[test]
    fn visible_pane_ids_ignores_stale_repo_filter() {
        let groups = vec![test_group_with_status(
            "app",
            &[("%1", crate::tmux::PaneStatus::Running)],
        )];
        assert_eq!(
            visible_pane_ids(
                &groups,
                StatusFilter::All,
                &RepoFilter::Repo("deleted".into())
            ),
            vec!["%1"]
        );
    }
}
