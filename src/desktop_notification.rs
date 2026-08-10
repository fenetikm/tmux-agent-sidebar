use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::process::Stdio;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::time::now_epoch_secs;
use crate::tmux;

pub(crate) const DESKTOP_NOTIFICATION_COOLDOWN_SECS: u64 = 120;
const DESKTOP_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(3);
const DESKTOP_NOTIFICATION_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopNotificationKind {
    TaskCompleted,
    TaskFailed,
    PermissionRequired,
}

impl DesktopNotificationKind {
    /// Every kind that writes a notification stamp. `focus notification`
    /// walks this list to find the newest stamp on a pane. This is a
    /// hand-written literal, not derived from the enum — when adding a
    /// variant, add it here too, or it stays invisible to `focus
    /// notification` with no test to catch the omission.
    pub const ALL: [Self; 3] = [
        Self::TaskCompleted,
        Self::TaskFailed,
        Self::PermissionRequired,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopNotificationEvent {
    Stop,
    Notification,
    TaskCompleted,
    StopFailure,
    PermissionDenied,
}

impl DesktopNotificationEvent {
    pub const ALL: [Self; 5] = [
        Self::Stop,
        Self::Notification,
        Self::TaskCompleted,
        Self::StopFailure,
        Self::PermissionDenied,
    ];

    pub const DEFAULT: [Self; 4] = [
        Self::Stop,
        Self::Notification,
        Self::StopFailure,
        Self::PermissionDenied,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Notification => "notification",
            Self::TaskCompleted => "task_completed",
            Self::StopFailure => "stop_failure",
            Self::PermissionDenied => "permission_denied",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "stop" => Some(Self::Stop),
            "notification" => Some(Self::Notification),
            "task_completed" => Some(Self::TaskCompleted),
            "stop_failure" => Some(Self::StopFailure),
            "permission_denied" => Some(Self::PermissionDenied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopNotificationBackend {
    Osascript,
    TerminalNotifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopNotificationSettings {
    pub enabled: bool,
    pub events: HashSet<DesktopNotificationEvent>,
    pub backend: DesktopNotificationBackend,
    pub icon: Option<String>,
    pub click_script: Option<String>,
    pub sound: Option<String>,
}

impl Default for DesktopNotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::Osascript,
            icon: None,
            click_script: None,
            sound: None,
        }
    }
}

impl DesktopNotificationSettings {
    pub fn from_tmux_options(opts: &HashMap<String, String>) -> Self {
        Self::from_tmux_options_with_backend(opts, notification_backend_available)
    }

    fn from_tmux_options_with_backend<A>(
        opts: &HashMap<String, String>,
        backend_available: A,
    ) -> Self
    where
        A: BackendAvailability,
    {
        let backend = parse_backend(opts.get(tmux::SIDEBAR_NOTIFICATIONS_BACKEND));
        let enabled = read_bool(opts, tmux::SIDEBAR_NOTIFICATIONS).unwrap_or(true);
        let backend_is_available = backend
            .map(|backend| backend_available.available_for(backend))
            .unwrap_or(false);
        let enabled = enabled && backend_is_available && backend.is_some();
        let events = opts
            .get(tmux::SIDEBAR_NOTIFICATIONS_EVENTS)
            .map_or_else(|| Self::default().events, |raw| parse_events(raw));

        Self {
            enabled,
            events,
            backend: backend.unwrap_or(DesktopNotificationBackend::Osascript),
            icon: read_non_empty_trimmed(opts, tmux::SIDEBAR_NOTIFICATIONS_ICON),
            click_script: read_non_empty_trimmed(opts, tmux::SIDEBAR_NOTIFICATIONS_CLICK_SCRIPT),
            sound: read_non_empty_trimmed(opts, tmux::SIDEBAR_NOTIFICATIONS_SOUND),
        }
    }

    pub fn from_tmux() -> Self {
        Self::from_tmux_options(&tmux::get_all_global_options())
    }

    pub fn event_enabled(&self, event: DesktopNotificationEvent) -> bool {
        self.events.contains(&event)
    }
}

trait BackendAvailability {
    fn available_for(self, backend: DesktopNotificationBackend) -> bool;
}

impl BackendAvailability for bool {
    fn available_for(self, _backend: DesktopNotificationBackend) -> bool {
        self
    }
}

impl<F> BackendAvailability for F
where
    F: FnOnce(DesktopNotificationBackend) -> bool,
{
    fn available_for(self, backend: DesktopNotificationBackend) -> bool {
        self(backend)
    }
}

fn parse_backend(raw: Option<&String>) -> Option<DesktopNotificationBackend> {
    match raw
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        None => Some(DesktopNotificationBackend::Osascript),
        Some(value) if value.eq_ignore_ascii_case("osascript") => {
            Some(DesktopNotificationBackend::Osascript)
        }
        Some(value) if value.eq_ignore_ascii_case("terminal-notifier") => {
            Some(DesktopNotificationBackend::TerminalNotifier)
        }
        Some(_) => None,
    }
}

fn read_non_empty_trimmed(opts: &HashMap<String, String>, key: &str) -> Option<String> {
    let value = opts.get(key)?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_events(raw: &str) -> HashSet<DesktopNotificationEvent> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return HashSet::new();
    }
    if trimmed.eq_ignore_ascii_case("all") {
        return DesktopNotificationEvent::ALL.iter().copied().collect();
    }
    trimmed
        .split(',')
        .filter_map(DesktopNotificationEvent::from_token)
        .collect()
}

pub fn format_title(repo: Option<&str>, branch: Option<&str>, agent: &str) -> String {
    let repo = repo.map(str::trim).filter(|s| !s.is_empty());
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());
    match (repo, branch) {
        (Some(repo), Some(branch)) => format!("{repo} ({branch}) / {agent}"),
        (Some(repo), None) => format!("{repo} / {agent}"),
        _ => agent.to_string(),
    }
}

pub fn run_scoped_fingerprint(started_at: Option<u64>, fingerprint: &str) -> String {
    match started_at {
        Some(started_at) => format!("{started_at}:{fingerprint}"),
        None => fingerprint.to_string(),
    }
}

/// Returns true if a notification of `kind` has already fired for the
/// current `run_id` on this pane. Use to dedupe events that share a kind
/// but use distinct fingerprints (e.g. `Stop` vs explicit `TaskCompleted`
/// in the same run).
pub fn has_run_scoped_stamp(
    pane_id: &str,
    kind: DesktopNotificationKind,
    run_id: Option<u64>,
) -> bool {
    let Some(run_id) = run_id else { return false };
    if pane_id.is_empty() {
        return false;
    }
    let raw = tmux::get_pane_option_value(pane_id, stamp_option_key(kind));
    let Some(stamp) = parse_stamp(&raw) else {
        return false;
    };
    stamp.fingerprint.starts_with(&format!("{run_id}:"))
}

pub fn notify_if_allowed(
    settings: &DesktopNotificationSettings,
    pane_id: &str,
    kind: DesktopNotificationKind,
    event: DesktopNotificationEvent,
    fingerprint: &str,
    title: &str,
    body: &str,
) -> bool {
    if !settings.enabled || pane_id.is_empty() || !settings.event_enabled(event) {
        return false;
    }

    let key = stamp_option_key(kind);
    let normalized_fingerprint = normalize_fingerprint(fingerprint);
    let now = now_epoch_secs();
    let current = tmux::get_pane_option_value(pane_id, key);
    if let Some(stamp) = parse_stamp(&current)
        && stamp.fingerprint == normalized_fingerprint
        && now.saturating_sub(stamp.timestamp) < DESKTOP_NOTIFICATION_COOLDOWN_SECS
    {
        return false;
    }

    let targets = notification_targets_for_settings(settings, pane_id);
    match send_desktop_notification(settings, &targets, title, body) {
        Ok(()) => {
            tmux::set_pane_option(pane_id, key, &encode_stamp(now, &normalized_fingerprint));
            true
        }
        Err(err) => {
            eprintln!("desktop notification failed: {err}");
            false
        }
    }
}

fn read_bool(opts: &HashMap<String, String>, key: &str) -> Option<bool> {
    let raw = opts.get(key)?.trim().to_ascii_lowercase();
    match raw.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

struct NotificationStamp {
    timestamp: u64,
    fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct NotificationTargets {
    pane_id: String,
    session_id: String,
    window_id: String,
}

fn notification_targets(pane_id: &str) -> NotificationTargets {
    notification_targets_with_display(pane_id, tmux::display_message)
}

fn notification_targets_for_settings(
    settings: &DesktopNotificationSettings,
    pane_id: &str,
) -> NotificationTargets {
    if settings.backend == DesktopNotificationBackend::TerminalNotifier
        && settings.click_script.is_some()
        && cfg!(target_os = "macos")
    {
        return notification_targets(pane_id);
    }

    NotificationTargets {
        pane_id: pane_id.to_string(),
        session_id: String::new(),
        window_id: String::new(),
    }
}

#[cfg(test)]
fn notification_targets_for_settings_with_display<F>(
    settings: &DesktopNotificationSettings,
    pane_id: &str,
    is_macos: bool,
    display_fn: F,
) -> NotificationTargets
where
    F: Fn(&str, &str) -> String,
{
    if settings.backend == DesktopNotificationBackend::TerminalNotifier
        && settings.click_script.is_some()
        && is_macos
    {
        return notification_targets_with_display(pane_id, display_fn);
    }

    NotificationTargets {
        pane_id: pane_id.to_string(),
        session_id: String::new(),
        window_id: String::new(),
    }
}

fn notification_targets_with_display<F>(pane_id: &str, display_fn: F) -> NotificationTargets
where
    F: Fn(&str, &str) -> String,
{
    NotificationTargets {
        pane_id: pane_id.to_string(),
        session_id: display_fn(pane_id, "#{session_id}"),
        window_id: display_fn(pane_id, "#{window_id}"),
    }
}

fn stamp_option_key(kind: DesktopNotificationKind) -> &'static str {
    match kind {
        DesktopNotificationKind::TaskCompleted => tmux::PANE_OS_NOTIFY_TASK_COMPLETED,
        DesktopNotificationKind::TaskFailed => tmux::PANE_OS_NOTIFY_TASK_FAILED,
        DesktopNotificationKind::PermissionRequired => tmux::PANE_OS_NOTIFY_PERMISSION_REQUIRED,
    }
}

/// The pane options carrying notification stamps, one per
/// [`DesktopNotificationKind`]. Read by `focus notification` to find the
/// most recently notified pane.
pub fn stamp_option_keys() -> [&'static str; 3] {
    DesktopNotificationKind::ALL.map(stamp_option_key)
}

/// Extract the epoch-seconds timestamp from a raw stamp option value
/// (`"<seconds>|<fingerprint>"`). Returns `None` for empty, malformed, or
/// non-numeric values so a corrupt or unset pane option is skipped rather
/// than treated as an ancient notification.
pub fn stamp_timestamp(raw: &str) -> Option<u64> {
    parse_stamp(raw).map(|stamp| stamp.timestamp)
}

fn encode_stamp(timestamp: u64, fingerprint: &str) -> String {
    format!("{}|{}", timestamp, fingerprint)
}

fn parse_stamp(raw: &str) -> Option<NotificationStamp> {
    let (ts, fingerprint) = raw.split_once('|')?;
    Some(NotificationStamp {
        timestamp: ts.parse().ok()?,
        fingerprint: fingerprint.to_string(),
    })
}

fn normalize_fingerprint(value: &str) -> String {
    value.replace(['|', '\n', '\r'], " ")
}

fn send_desktop_notification(
    settings: &DesktopNotificationSettings,
    targets: &NotificationTargets,
    title: &str,
    body: &str,
) -> Result<(), String> {
    // Unit tests exercise the full notify path (gate, fingerprint, stamp write)
    // but must not spawn osascript/notify-send on the developer's machine.
    #[cfg(test)]
    {
        let _ = (settings, targets, title, body);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        match settings.backend {
            DesktopNotificationBackend::Osascript => {
                let script = build_osascript_script(title, body, settings.sound.as_deref());
                let mut command = Command::new("osascript");
                command
                    .args(["-e", &script])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                run_notification_command(&mut command, "osascript", DESKTOP_NOTIFICATION_TIMEOUT)
            }
            DesktopNotificationBackend::TerminalNotifier => {
                let args = build_terminal_notifier_args(title, body, settings, targets);
                let mut command = Command::new("terminal-notifier");
                command
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                run_notification_command(
                    &mut command,
                    "terminal-notifier",
                    DESKTOP_NOTIFICATION_TIMEOUT,
                )
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (settings, targets);
        let mut command = Command::new("notify-send");
        command
            .args([
                "--app-name=tmux-agent-sidebar",
                "--urgency=normal",
                title,
                body,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        run_notification_command(&mut command, "notify-send", DESKTOP_NOTIFICATION_TIMEOUT)
    }

    #[cfg(target_os = "windows")]
    {
        let _ = (settings, targets, title, body);
        Err("desktop notifications are not supported on Windows yet".into())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (settings, targets, title, body);
        Err("desktop notifications are not supported on this platform".into())
    }
}

fn notification_backend_available(backend: DesktopNotificationBackend) -> bool {
    #[cfg(target_os = "macos")]
    {
        match backend {
            DesktopNotificationBackend::Osascript => {
                let mut command = Command::new("osascript");
                command
                    .args(["-e", "return 0"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                run_notification_command(
                    &mut command,
                    "osascript",
                    DESKTOP_NOTIFICATION_PROBE_TIMEOUT,
                )
                .is_ok()
            }
            DesktopNotificationBackend::TerminalNotifier => {
                let mut command = Command::new("terminal-notifier");
                command
                    .arg("-help")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                run_notification_command(
                    &mut command,
                    "terminal-notifier",
                    DESKTOP_NOTIFICATION_PROBE_TIMEOUT,
                )
                .is_ok()
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = backend;
        let mut command = Command::new("notify-send");
        command
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        return run_notification_command(
            &mut command,
            "notify-send",
            DESKTOP_NOTIFICATION_PROBE_TIMEOUT,
        )
        .is_ok();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = backend;
        false
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = backend;
        false
    }
}

fn escape_applescript(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[allow(dead_code)]
fn build_osascript_script(title: &str, body: &str, sound: Option<&str>) -> String {
    let mut script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(body),
        escape_applescript(title)
    );
    if let Some(sound) = sound.map(str::trim).filter(|value| !value.is_empty()) {
        script.push_str(&format!(" sound name \"{}\"", escape_applescript(sound)));
    }
    script
}

#[allow(dead_code)]
fn build_terminal_notifier_args(
    title: &str,
    body: &str,
    settings: &DesktopNotificationSettings,
    targets: &NotificationTargets,
) -> Vec<String> {
    let mut args = vec![
        "-title".to_string(),
        title.to_string(),
        "-message".to_string(),
        body.to_string(),
    ];

    if let Some(icon) = &settings.icon {
        args.push("-appIcon".to_string());
        args.push(icon.clone());
    }
    if let Some(click_script) = &settings.click_script {
        args.push("-execute".to_string());
        args.push(build_click_script_command(click_script, targets));
    }
    if let Some(sound) = &settings.sound {
        args.push("-sound".to_string());
        args.push(sound.clone());
    }

    args
}

#[allow(dead_code)]
fn build_click_script_command(script: &str, targets: &NotificationTargets) -> String {
    [
        shell_quote(script),
        shell_quote(&targets.pane_id),
        shell_quote(&targets.session_id),
        shell_quote(&targets.window_id),
    ]
    .join(" ")
}

#[allow(dead_code)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_notification_command(
    command: &mut Command,
    command_name: &str,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to spawn {command_name}: {err}"))?;
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("{command_name} exited with status {status}"));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{command_name} timed out after {}s",
                        timeout.as_secs()
                    ));
                }
                sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(format!("failed to wait on {command_name}: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn settings_parse_bool_and_numbers() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS.into(), "on".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert!(settings.enabled);
    }

    #[test]
    fn settings_default_when_invalid() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS.into(), "maybe".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert!(settings.enabled);
    }

    #[test]
    fn settings_disable_when_off() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS.into(), "off".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert!(!settings.enabled);
    }

    #[test]
    fn settings_disable_when_backend_missing() {
        let opts = HashMap::new();
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, false);
        assert!(!settings.enabled);
    }

    #[test]
    fn settings_default_backend_is_osascript() {
        let opts = HashMap::new();
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert_eq!(settings.backend, DesktopNotificationBackend::Osascript);
        assert!(settings.enabled);
    }

    #[test]
    fn settings_parse_explicit_osascript_backend() {
        let mut opts = HashMap::new();
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_BACKEND.into(),
            "osascript".into(),
        );

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);

        assert_eq!(settings.backend, DesktopNotificationBackend::Osascript);
        assert!(settings.enabled);
    }

    #[test]
    fn settings_empty_backend_defaults_to_osascript() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_BACKEND.into(), " \t\n ".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);

        assert_eq!(settings.backend, DesktopNotificationBackend::Osascript);
        assert!(settings.enabled);
    }

    #[test]
    fn settings_parse_terminal_notifier_backend_and_advanced_options() {
        let mut opts = HashMap::new();
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_BACKEND.into(),
            " terminal-notifier ".into(),
        );
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_ICON.into(),
            " /tmp/icon.png ".into(),
        );
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_CLICK_SCRIPT.into(),
            " ~/bin/click.sh ".into(),
        );
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_SOUND.into(), " Glass ".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);

        assert_eq!(
            settings.backend,
            DesktopNotificationBackend::TerminalNotifier
        );
        assert_eq!(settings.icon.as_deref(), Some("/tmp/icon.png"));
        assert_eq!(settings.click_script.as_deref(), Some("~/bin/click.sh"));
        assert_eq!(settings.sound.as_deref(), Some("Glass"));
        assert!(settings.enabled);
    }

    #[test]
    fn settings_trim_empty_advanced_options() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_ICON.into(), "   ".into());
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_CLICK_SCRIPT.into(),
            "\t\n".into(),
        );
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_SOUND.into(), "".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);

        assert_eq!(settings.icon, None);
        assert_eq!(settings.click_script, None);
        assert_eq!(settings.sound, None);
    }

    #[test]
    fn settings_unknown_backend_disables_notifications() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_BACKEND.into(), "bogus".into());

        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);

        assert!(!settings.enabled);
        assert_eq!(settings.backend, DesktopNotificationBackend::Osascript);
    }

    #[test]
    fn settings_probe_selected_backend() {
        let mut opts = HashMap::new();
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_BACKEND.into(),
            "terminal-notifier".into(),
        );
        let settings =
            DesktopNotificationSettings::from_tmux_options_with_backend(&opts, |backend| {
                assert_eq!(backend, DesktopNotificationBackend::TerminalNotifier);
                false
            });

        assert_eq!(
            settings.backend,
            DesktopNotificationBackend::TerminalNotifier
        );
        assert!(!settings.enabled);
    }

    #[test]
    fn notification_targets_resolve_session_and_window_from_pane() {
        let targets = notification_targets_with_display("%12", |pane_id, template| {
            assert_eq!(pane_id, "%12");
            match template {
                "#{session_id}" => "$3".to_string(),
                "#{window_id}" => "@7".to_string(),
                _ => panic!("unexpected template {template}"),
            }
        });

        assert_eq!(
            targets,
            NotificationTargets {
                pane_id: "%12".into(),
                session_id: "$3".into(),
                window_id: "@7".into(),
            }
        );
    }

    #[test]
    fn notification_targets_for_settings_skips_display_without_click_script() {
        let settings = DesktopNotificationSettings {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::TerminalNotifier,
            icon: None,
            click_script: None,
            sound: None,
        };

        let targets =
            notification_targets_for_settings_with_display(&settings, "%12", true, |_, _| {
                panic!("display-message should not be called without click_script")
            });

        assert_eq!(
            targets,
            NotificationTargets {
                pane_id: "%12".into(),
                session_id: String::new(),
                window_id: String::new(),
            }
        );
    }

    #[test]
    fn notification_targets_for_settings_skips_display_for_non_macos_click_script() {
        let settings = DesktopNotificationSettings {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::TerminalNotifier,
            icon: None,
            click_script: Some("/tmp/focus.sh".into()),
            sound: None,
        };

        let targets =
            notification_targets_for_settings_with_display(&settings, "%12", false, |_, _| {
                panic!("display-message should not be called off macOS")
            });

        assert_eq!(
            targets,
            NotificationTargets {
                pane_id: "%12".into(),
                session_id: String::new(),
                window_id: String::new(),
            }
        );
    }

    #[test]
    fn notification_targets_for_settings_resolves_display_for_macos_terminal_notifier_click_script()
    {
        let settings = DesktopNotificationSettings {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::TerminalNotifier,
            icon: None,
            click_script: Some("/tmp/focus.sh".into()),
            sound: None,
        };
        let calls = Cell::new(0);

        let targets = notification_targets_for_settings_with_display(
            &settings,
            "%12",
            true,
            |pane_id, template| {
                assert_eq!(pane_id, "%12");
                calls.set(calls.get() + 1);
                match template {
                    "#{session_id}" => "$3".to_string(),
                    "#{window_id}" => "@7".to_string(),
                    _ => panic!("unexpected template {template}"),
                }
            },
        );

        assert_eq!(calls.get(), 2);
        assert_eq!(
            targets,
            NotificationTargets {
                pane_id: "%12".into(),
                session_id: "$3".into(),
                window_id: "@7".into(),
            }
        );
    }

    #[test]
    fn format_title_variants() {
        assert_eq!(
            format_title(Some("repo"), Some("feat/xyz"), "claude"),
            "repo (feat/xyz) / claude"
        );
        assert_eq!(format_title(Some("repo"), None, "claude"), "repo / claude");
        assert_eq!(
            format_title(Some("repo"), Some(""), "claude"),
            "repo / claude"
        );
        assert_eq!(format_title(None, Some("feat"), "claude"), "claude");
        assert_eq!(format_title(None, None, "claude"), "claude");
    }

    #[test]
    fn stamp_round_trip() {
        let stamp = encode_stamp(123, "foo bar");
        let parsed = parse_stamp(&stamp).unwrap();
        assert_eq!(parsed.timestamp, 123);
        assert_eq!(parsed.fingerprint, "foo bar");
    }

    #[test]
    fn fingerprint_is_normalized() {
        assert_eq!(
            normalize_fingerprint("foo|bar\nbaz\rqux"),
            "foo bar baz qux"
        );
    }

    #[test]
    fn osascript_script_includes_sound_when_configured() {
        assert_eq!(
            build_osascript_script("Title", "Body", Some("Frog")),
            "display notification \"Body\" with title \"Title\" sound name \"Frog\""
        );
    }

    #[test]
    fn osascript_script_omits_sound_when_unset() {
        assert_eq!(
            build_osascript_script("Title", "Body", None),
            "display notification \"Body\" with title \"Title\""
        );
    }

    #[test]
    fn osascript_script_escapes_values() {
        assert_eq!(
            build_osascript_script("A \"title\"", "Body\\line\nnext", Some("Ping\"Pong")),
            "display notification \"Body\\\\line next\" with title \"A \\\"title\\\"\" sound name \"Ping\\\"Pong\""
        );
    }

    #[test]
    fn terminal_notifier_args_include_configured_options() {
        let settings = DesktopNotificationSettings {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::TerminalNotifier,
            icon: Some("/tmp/icon.png".into()),
            click_script: Some("/tmp/focus pane.sh".into()),
            sound: Some("default".into()),
        };
        let targets = NotificationTargets {
            pane_id: "%12".into(),
            session_id: "$3".into(),
            window_id: "@7".into(),
        };

        assert_eq!(
            build_terminal_notifier_args("Title", "Body", &settings, &targets),
            vec![
                "-title",
                "Title",
                "-message",
                "Body",
                "-appIcon",
                "/tmp/icon.png",
                "-execute",
                "'/tmp/focus pane.sh' '%12' '$3' '@7'",
                "-sound",
                "default",
            ]
        );
    }

    #[test]
    fn terminal_notifier_args_omit_unset_options() {
        let settings = DesktopNotificationSettings {
            enabled: true,
            events: DesktopNotificationEvent::DEFAULT.iter().copied().collect(),
            backend: DesktopNotificationBackend::TerminalNotifier,
            icon: None,
            click_script: None,
            sound: None,
        };
        let targets = NotificationTargets {
            pane_id: "%12".into(),
            session_id: "$3".into(),
            window_id: "@7".into(),
        };

        assert_eq!(
            build_terminal_notifier_args("Title", "Body", &settings, &targets),
            vec!["-title", "Title", "-message", "Body"]
        );
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/it's fine.sh"), "'/tmp/it'\\''s fine.sh'");
    }

    #[test]
    fn click_script_command_shell_quotes_script_and_targets() {
        let targets = NotificationTargets {
            pane_id: "%12".into(),
            session_id: "$3".into(),
            window_id: "@7".into(),
        };

        assert_eq!(
            build_click_script_command("/tmp/it's fine.sh", &targets),
            "'/tmp/it'\\''s fine.sh' '%12' '$3' '@7'"
        );
    }

    #[test]
    fn events_default_to_default_set_when_unset() {
        let opts = HashMap::new();
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        for event in DesktopNotificationEvent::DEFAULT {
            assert!(settings.event_enabled(event), "expected {event:?} enabled");
        }
        assert!(
            !settings.event_enabled(DesktopNotificationEvent::TaskCompleted),
            "task_completed should be opt-in"
        );
    }

    #[test]
    fn events_parse_explicit_subset() {
        let mut opts = HashMap::new();
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_EVENTS.into(),
            "stop, permission_denied".into(),
        );
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert!(settings.event_enabled(DesktopNotificationEvent::Stop));
        assert!(settings.event_enabled(DesktopNotificationEvent::PermissionDenied));
        assert!(!settings.event_enabled(DesktopNotificationEvent::Notification));
        assert!(!settings.event_enabled(DesktopNotificationEvent::TaskCompleted));
        assert!(!settings.event_enabled(DesktopNotificationEvent::StopFailure));
    }

    #[test]
    fn events_all_keyword_enables_every_event() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_EVENTS.into(), "all".into());
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        for event in DesktopNotificationEvent::ALL {
            assert!(settings.event_enabled(event));
        }
    }

    #[test]
    fn events_empty_value_disables_every_event() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_NOTIFICATIONS_EVENTS.into(), "".into());
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        for event in DesktopNotificationEvent::ALL {
            assert!(!settings.event_enabled(event));
        }
    }

    #[test]
    fn events_unknown_tokens_are_ignored() {
        let mut opts = HashMap::new();
        opts.insert(
            tmux::SIDEBAR_NOTIFICATIONS_EVENTS.into(),
            "stop,bogus, task_completed".into(),
        );
        let settings = DesktopNotificationSettings::from_tmux_options_with_backend(&opts, true);
        assert!(settings.event_enabled(DesktopNotificationEvent::Stop));
        assert!(settings.event_enabled(DesktopNotificationEvent::TaskCompleted));
        assert!(!settings.event_enabled(DesktopNotificationEvent::Notification));
    }

    #[test]
    fn has_run_scoped_stamp_returns_false_without_stamp() {
        let _guard = tmux::test_mock::install();
        let pane = "%PANE_NO_STAMP";
        assert!(!has_run_scoped_stamp(
            pane,
            DesktopNotificationKind::TaskCompleted,
            Some(1_700_000_000_000),
        ));
    }

    #[test]
    fn has_run_scoped_stamp_matches_current_run() {
        let _guard = tmux::test_mock::install();
        let pane = "%PANE_CURRENT_RUN";
        let run_id = 1_700_000_000_000_u64;
        let stamp = encode_stamp(42, &format!("{run_id}:task-xyz"));
        tmux::test_mock::set(
            pane,
            stamp_option_key(DesktopNotificationKind::TaskCompleted),
            &stamp,
        );
        assert!(has_run_scoped_stamp(
            pane,
            DesktopNotificationKind::TaskCompleted,
            Some(run_id),
        ));
    }

    #[test]
    fn has_run_scoped_stamp_rejects_stale_run() {
        let _guard = tmux::test_mock::install();
        let pane = "%PANE_STALE_RUN";
        let old_run = 1_600_000_000_000_u64;
        let new_run = 1_700_000_000_000_u64;
        let stamp = encode_stamp(42, &format!("{old_run}:task-xyz"));
        tmux::test_mock::set(
            pane,
            stamp_option_key(DesktopNotificationKind::TaskCompleted),
            &stamp,
        );
        assert!(!has_run_scoped_stamp(
            pane,
            DesktopNotificationKind::TaskCompleted,
            Some(new_run),
        ));
    }

    #[test]
    fn has_run_scoped_stamp_requires_run_id() {
        let _guard = tmux::test_mock::install();
        let pane = "%PANE_NO_RUN_ID";
        let stamp = encode_stamp(42, "1700000000000:task-xyz");
        tmux::test_mock::set(
            pane,
            stamp_option_key(DesktopNotificationKind::TaskCompleted),
            &stamp,
        );
        assert!(!has_run_scoped_stamp(
            pane,
            DesktopNotificationKind::TaskCompleted,
            None,
        ));
    }

    #[test]
    fn stamp_option_keys_covers_every_notification_kind() {
        assert_eq!(
            stamp_option_keys(),
            [
                tmux::PANE_OS_NOTIFY_TASK_COMPLETED,
                tmux::PANE_OS_NOTIFY_TASK_FAILED,
                tmux::PANE_OS_NOTIFY_PERMISSION_REQUIRED,
            ]
        );
    }

    #[test]
    fn stamp_timestamp_reads_the_leading_seconds_field() {
        // Real stored shape: "<seconds>|<run_id>:<fingerprint>".
        assert_eq!(
            stamp_timestamp("1700000123|1699999999:notification"),
            Some(1_700_000_123)
        );
    }

    #[test]
    fn stamp_timestamp_rejects_unusable_values() {
        assert_eq!(stamp_timestamp(""), None);
        assert_eq!(stamp_timestamp("no-separator"), None);
        assert_eq!(stamp_timestamp("notanumber|fingerprint"), None);
        assert_eq!(stamp_timestamp("|fingerprint"), None);
    }
}
