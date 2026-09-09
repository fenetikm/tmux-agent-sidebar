//! Custom bottom panel: row types, NDJSON parsing, command execution and
//! the per-process result cache.
//!
//! This module is deliberately free of ratatui, tmux and `AppState` types so
//! parsing and colour naming can be unit-tested without rendering a frame.
//! Mapping a [`PanelColor`] onto a concrete terminal colour is the UI
//! layer's job (`src/ui/bottom/panel.rs`).

use std::collections::HashMap;
use std::time::{Duration, Instant};
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

const DEFAULT_NAME: &str = "Custom";
const DEFAULT_INTERVAL: Duration = Duration::from_secs(120);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolved `@sidebar_panel_*` configuration. Its existence is what enables
/// the feature: `None` means no tab, no worker, no subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelConfig {
    pub command: String,
    pub name: String,
    pub interval: Duration,
    pub timeout: Duration,
}

/// Parse a positive whole number of seconds, falling back to `default` for
/// missing, malformed or zero values — matching how `@sidebar_sorting`
/// treats unrecognised input.
fn secs_or(opts: &HashMap<String, String>, key: &str, default: Duration) -> Duration {
    opts.get(key)
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
}

impl PanelConfig {
    pub fn from_options(opts: &HashMap<String, String>) -> Option<Self> {
        let command = opts
            .get(crate::tmux::SIDEBAR_PANEL_COMMAND)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let name = opts
            .get(crate::tmux::SIDEBAR_PANEL_NAME)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_NAME.to_string());
        Some(Self {
            command,
            name,
            interval: secs_or(opts, crate::tmux::SIDEBAR_PANEL_INTERVAL, DEFAULT_INTERVAL),
            timeout: secs_or(opts, crate::tmux::SIDEBAR_PANEL_TIMEOUT, DEFAULT_TIMEOUT),
        })
    }
}

/// Context handed to a panel script, mirroring the focused pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelContext {
    /// Resolved repository root, used as both cwd and `SIDEBAR_REPO_PATH`.
    pub repo_path: String,
    pub branch: String,
    /// Focused pane's tmux ID. Scripts use it to reach any per-pane option
    /// (`tmux show -pt "$SIDEBAR_PANE_ID" -v @pane_agent`), which is why no
    /// individual pane field gets its own variable.
    pub pane_id: String,
    pub session: String,
}

/// One run's worth of panel output.
#[derive(Debug, Clone)]
pub struct PanelData {
    pub rows: Vec<PanelRow>,
    pub error: Option<String>,
    pub fetched_at: Instant,
}

/// Run the configured command and parse its output.
///
/// Errors are returned as data rather than propagated: the panel keeps
/// showing whatever it last had, with the error rendered beneath it.
pub fn run_command(config: &PanelConfig, ctx: &PanelContext) -> PanelData {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(&config.command)
        .current_dir(&ctx.repo_path)
        .env("SIDEBAR_REPO_PATH", &ctx.repo_path)
        .env("SIDEBAR_BRANCH", &ctx.branch)
        .env("SIDEBAR_PANE_ID", &ctx.pane_id)
        .env("SIDEBAR_SESSION", &ctx.session);

    let (rows, error) = match crate::process::run_with_deadline(&mut cmd, config.timeout) {
        crate::process::RunOutcome::Completed(out) if out.status.success() => {
            parse_rows(&String::from_utf8_lossy(&out.stdout))
        }
        crate::process::RunOutcome::Completed(out) => {
            let code = out
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().find(|l| !l.trim().is_empty());
            let message = match first {
                Some(line) => format!("exit {code}: {}", line.trim()),
                None => format!("exit {code}"),
            };
            (Vec::new(), Some(message))
        }
        crate::process::RunOutcome::TimedOut => (
            Vec::new(),
            Some(format!("timed out after {}s", config.timeout.as_secs())),
        ),
        crate::process::RunOutcome::SpawnFailed(err) => {
            (Vec::new(), Some(format!("failed to start: {err}")))
        }
    };

    PanelData {
        rows,
        error,
        fetched_at: Instant::now(),
    }
}

/// Per-process cache of panel results, keyed by repository path.
///
/// Exists to absorb focus thrash rather than to save API quota:
/// cross-process deduplication is the script's job. Multi-entry so moving
/// between two repositories does not evict either one.
#[derive(Debug, Default)]
pub struct PanelCache {
    entries: HashMap<String, PanelData>,
}

impl PanelCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached result for `key` when it is younger than `ttl`,
    /// otherwise call `run` and store the result.
    ///
    /// A failed run keeps the previous rows so the panel does not flicker
    /// between useful and empty on a transient error. A *successful* run
    /// returning nothing does clear them — that is a real result.
    pub fn get_or_run<F>(&mut self, key: &str, now: Instant, ttl: Duration, run: F) -> PanelData
    where
        F: FnOnce() -> PanelData,
    {
        if let Some(cached) = self.entries.get(key)
            && now.duration_since(cached.fetched_at) < ttl
        {
            return cached.clone();
        }

        let mut fresh = run();
        if fresh.error.is_some()
            && fresh.rows.is_empty()
            && let Some(previous) = self.entries.get(key)
        {
            fresh.rows = previous.rows.clone();
        }
        self.entries.insert(key.to_string(), fresh.clone());
        fresh
    }
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

    use std::collections::HashMap;
    use std::time::Duration;

    fn opts(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn config_is_none_without_a_command() {
        assert!(PanelConfig::from_options(&opts(&[("@sidebar_panel_name", "PRs")])).is_none());
    }

    #[test]
    fn config_is_none_for_a_blank_command() {
        assert!(PanelConfig::from_options(&opts(&[("@sidebar_panel_command", "   ")])).is_none());
    }

    #[test]
    fn config_uses_defaults() {
        let cfg = PanelConfig::from_options(&opts(&[("@sidebar_panel_command", "echo hi")]))
            .expect("command set");
        assert_eq!(cfg.command, "echo hi");
        assert_eq!(cfg.name, "Custom");
        assert_eq!(cfg.interval, Duration::from_secs(120));
        assert_eq!(cfg.timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_reads_all_overrides() {
        let cfg = PanelConfig::from_options(&opts(&[
            ("@sidebar_panel_command", "gh pr list"),
            ("@sidebar_panel_name", "PRs"),
            ("@sidebar_panel_interval", "30"),
            ("@sidebar_panel_timeout", "3"),
        ]))
        .expect("command set");
        assert_eq!(cfg.name, "PRs");
        assert_eq!(cfg.interval, Duration::from_secs(30));
        assert_eq!(cfg.timeout, Duration::from_secs(3));
    }

    #[test]
    fn malformed_or_zero_numbers_fall_back_to_defaults() {
        let cfg = PanelConfig::from_options(&opts(&[
            ("@sidebar_panel_command", "echo hi"),
            ("@sidebar_panel_interval", "abc"),
            ("@sidebar_panel_timeout", "0"),
        ]))
        .expect("command set");
        assert_eq!(cfg.interval, Duration::from_secs(120));
        assert_eq!(cfg.timeout, Duration::from_secs(10));
    }

    #[test]
    fn blank_name_falls_back_to_default() {
        let cfg = PanelConfig::from_options(&opts(&[
            ("@sidebar_panel_command", "echo hi"),
            ("@sidebar_panel_name", "  "),
        ]))
        .expect("command set");
        assert_eq!(cfg.name, "Custom");
    }

    fn cfg(command: &str) -> PanelConfig {
        PanelConfig {
            command: command.to_string(),
            name: "Custom".into(),
            interval: Duration::from_secs(120),
            timeout: Duration::from_secs(5),
        }
    }

    fn ctx() -> PanelContext {
        PanelContext {
            repo_path: "/tmp".into(),
            branch: "main".into(),
            pane_id: "%1".into(),
            session: "work".into(),
        }
    }

    #[test]
    fn runs_the_command_and_parses_rows() {
        let data = run_command(&cfg(r#"printf '{"text":"ok"}\n'"#), &ctx());
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].text, "ok");
        assert!(data.error.is_none());
    }

    #[test]
    fn exposes_context_in_the_environment() {
        let data = run_command(
            &cfg(
                r#"printf '{"text":"%s %s %s %s"}\n' "$SIDEBAR_REPO_PATH" "$SIDEBAR_BRANCH" "$SIDEBAR_PANE_ID" "$SIDEBAR_SESSION""#,
            ),
            &ctx(),
        );
        assert_eq!(data.rows[0].text, "/tmp main %1 work");
    }

    #[test]
    fn does_not_set_a_sidebar_agent_variable() {
        // Agent type is reachable via `tmux show -pt "$SIDEBAR_PANE_ID" -v
        // @pane_agent`; promoting one of 22 pane options to an env var is
        // deliberately not done.
        let data = run_command(
            &cfg(r#"printf '{"text":"[%s]"}\n' "$SIDEBAR_AGENT""#),
            &ctx(),
        );
        assert_eq!(data.rows[0].text, "[]");
    }

    #[test]
    fn runs_in_the_repo_path() {
        let data = run_command(&cfg(r#"printf '{"text":"%s"}\n' "$(pwd)""#), &ctx());
        // macOS reports /tmp as a symlink to /private/tmp; accept either.
        assert!(
            data.rows[0].text.ends_with("/tmp"),
            "unexpected cwd: {}",
            data.rows[0].text
        );
    }

    #[test]
    fn non_zero_exit_reports_the_first_stderr_line() {
        let data = run_command(
            &cfg("printf 'gh: not found\\nsecond line\\n' >&2; exit 1"),
            &ctx(),
        );
        assert!(data.rows.is_empty());
        assert_eq!(data.error.as_deref(), Some("exit 1: gh: not found"));
    }

    #[test]
    fn non_zero_exit_without_stderr_still_reports() {
        let data = run_command(&cfg("exit 2"), &ctx());
        assert_eq!(data.error.as_deref(), Some("exit 2"));
    }

    #[test]
    fn timeout_is_reported() {
        let mut config = cfg("sleep 5");
        config.timeout = Duration::from_millis(200);
        let data = run_command(&config, &ctx());
        assert_eq!(data.error.as_deref(), Some("timed out after 0s"));
    }

    #[test]
    fn oversized_output_surfaces_as_an_error_not_truncated_rows() {
        // A command that emits far more than `process::MAX_OUTPUT_BYTES`
        // (1 MiB) must never turn into a plausible-looking short row list
        // from a silently truncated NDJSON prefix — it must show up as an
        // error in the panel's footer instead, same as any other failure.
        let mut config = cfg("yes | head -c 20000000");
        config.timeout = Duration::from_secs(2);
        let data = run_command(&config, &ctx());
        assert!(
            data.rows.is_empty(),
            "oversized output must not parse into rows"
        );
        assert!(
            data.error.is_some(),
            "oversized output must surface as an error"
        );
    }

    #[test]
    fn successful_run_with_no_output_is_not_an_error() {
        let data = run_command(&cfg("true"), &ctx());
        assert!(data.rows.is_empty());
        assert!(data.error.is_none());
    }

    #[test]
    fn plain_text_output_is_an_error() {
        let data = run_command(&cfg("echo hello world"), &ctx());
        assert!(data.rows.is_empty());
        assert!(data.error.is_some());
    }

    fn data(texts: &[&str], error: Option<&str>, at: Instant) -> PanelData {
        PanelData {
            rows: texts
                .iter()
                .map(|t| PanelRow {
                    text: (*t).to_string(),
                    text_color: PanelColor::Default,
                    icon: None,
                    icon_color: PanelColor::Default,
                    url: None,
                    heading: false,
                })
                .collect(),
            error: error.map(str::to_string),
            fetched_at: at,
        }
    }

    #[test]
    fn first_call_runs_the_command() {
        let mut cache = PanelCache::new();
        let now = Instant::now();
        let mut ran = 0;
        let out = cache.get_or_run("/repo", now, Duration::from_secs(60), || {
            ran += 1;
            data(&["a"], None, now)
        });
        assert_eq!(ran, 1);
        assert_eq!(out.rows[0].text, "a");
    }

    #[test]
    fn fresh_entry_is_reused_without_running() {
        let mut cache = PanelCache::new();
        let t0 = Instant::now();
        cache.get_or_run("/repo", t0, Duration::from_secs(60), || {
            data(&["a"], None, t0)
        });

        let mut ran = 0;
        let out = cache.get_or_run(
            "/repo",
            t0 + Duration::from_secs(5),
            Duration::from_secs(60),
            || {
                ran += 1;
                data(&["b"], None, t0)
            },
        );
        assert_eq!(ran, 0, "within TTL the command must not run");
        assert_eq!(out.rows[0].text, "a");
    }

    #[test]
    fn expired_entry_reruns() {
        let mut cache = PanelCache::new();
        let t0 = Instant::now();
        cache.get_or_run("/repo", t0, Duration::from_secs(60), || {
            data(&["a"], None, t0)
        });

        let later = t0 + Duration::from_secs(61);
        let out = cache.get_or_run("/repo", later, Duration::from_secs(60), || {
            data(&["b"], None, later)
        });
        assert_eq!(out.rows[0].text, "b");
    }

    #[test]
    fn distinct_keys_do_not_evict_each_other() {
        let mut cache = PanelCache::new();
        let t0 = Instant::now();
        cache.get_or_run("/a", t0, Duration::from_secs(60), || {
            data(&["from a"], None, t0)
        });
        cache.get_or_run("/b", t0, Duration::from_secs(60), || {
            data(&["from b"], None, t0)
        });

        let mut ran = 0;
        let out = cache.get_or_run(
            "/a",
            t0 + Duration::from_secs(1),
            Duration::from_secs(60),
            || {
                ran += 1;
                data(&["rerun"], None, t0)
            },
        );
        assert_eq!(ran, 0, "/b must not have evicted /a");
        assert_eq!(out.rows[0].text, "from a");
    }

    #[test]
    fn failed_rerun_keeps_the_last_good_rows() {
        let mut cache = PanelCache::new();
        let t0 = Instant::now();
        cache.get_or_run("/repo", t0, Duration::from_secs(60), || {
            data(&["good"], None, t0)
        });

        let later = t0 + Duration::from_secs(61);
        let out = cache.get_or_run("/repo", later, Duration::from_secs(60), || {
            data(&[], Some("exit 1: boom"), later)
        });
        assert_eq!(out.rows[0].text, "good", "stale rows survive a failure");
        assert_eq!(out.error.as_deref(), Some("exit 1: boom"));
    }

    #[test]
    fn successful_empty_rerun_clears_the_rows() {
        let mut cache = PanelCache::new();
        let t0 = Instant::now();
        cache.get_or_run("/repo", t0, Duration::from_secs(60), || {
            data(&["good"], None, t0)
        });

        let later = t0 + Duration::from_secs(61);
        let out = cache.get_or_run("/repo", later, Duration::from_secs(60), || {
            data(&[], None, later)
        });
        assert!(out.rows.is_empty(), "an empty success is a real result");
        assert!(out.error.is_none());
    }
}
