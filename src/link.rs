//! Opening a clicked hyperlink in the user's browser.
//!
//! The sidebar renders links as OSC 8 escapes and normally leaves activation
//! to the terminal. That only works when the terminal can see the click, and
//! the sidebar holds the mouse grab, so a plain click never reaches it. This
//! module is the fallback: resolve a clicked cell to a URL and hand it to the
//! platform opener.

#[cfg(not(test))]
use std::process::{Command, Stdio};

/// The command used to open a URL: the configured override when it is set to
/// something non-blank, otherwise the platform default.
pub fn opener_command(configured: Option<&str>) -> String {
    match configured.map(str::trim) {
        Some(cmd) if !cmd.is_empty() => cmd.to_string(),
        _ => default_opener().to_string(),
    }
}

fn default_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    }
}

/// Hand `url` to the opener and return immediately.
///
/// The child is deliberately not waited on: a browser launch can take
/// seconds, and this runs on the UI thread. Its streams are detached so a
/// chatty opener cannot scribble over the rendered frame.
pub fn open(url: &str, configured: Option<&str>) -> Result<(), String> {
    // Unit tests exercise the click path but must not launch a browser on
    // the developer's machine.
    #[cfg(test)]
    {
        let _ = (url, configured);
        Ok(())
    }

    #[cfg(not(test))]
    {
        let program = opener_command(configured);
        Command::new(&program)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("failed to spawn {program}: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_defaults_to_the_platform_command() {
        let expected = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        assert_eq!(opener_command(None), expected);
    }

    #[test]
    fn opener_uses_a_configured_override() {
        assert_eq!(opener_command(Some("firefox")), "firefox");
    }

    #[test]
    fn opener_ignores_a_blank_override() {
        let expected = opener_command(None);
        assert_eq!(opener_command(Some("   ")), expected);
    }

    #[test]
    fn opening_is_stubbed_under_test_and_reports_success() {
        assert!(open("https://example.com", None).is_ok());
    }
}
