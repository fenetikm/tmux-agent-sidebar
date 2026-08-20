use crate::tmux;

/// Read `@sidebar_exclude_sessions` and split into whitespace-separated
/// glob patterns. Unset/empty => empty vec => nothing excluded.
#[allow(dead_code)]
pub(crate) fn exclude_session_patterns() -> Vec<String> {
    split_patterns(tmux::SIDEBAR_EXCLUDE_SESSIONS)
}

/// Read `@sidebar_exclude_windows` and split into whitespace-separated
/// glob patterns. Unset/empty => empty vec => nothing excluded.
#[allow(dead_code)]
pub(crate) fn exclude_window_patterns() -> Vec<String> {
    split_patterns(tmux::SIDEBAR_EXCLUDE_WINDOWS)
}

/// Read a tmux option and split it into whitespace-separated glob patterns.
fn split_patterns(option: &str) -> Vec<String> {
    tmux::get_option(option)
        .map(|value| split_pattern_value(&value))
        .unwrap_or_default()
}

/// Split a raw option value into whitespace-separated glob patterns. Shared
/// with callers that already hold the option value (e.g. the notification
/// settings, which read every global option in one batch).
pub(crate) fn split_pattern_value(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

/// True if `session_name` matches any session blocklist pattern.
#[allow(dead_code)]
pub(crate) fn session_excluded(session_name: &str, patterns: &[String]) -> bool {
    name_excluded(session_name, patterns)
}

/// True if `window_name` matches any window blocklist pattern.
#[allow(dead_code)]
pub(crate) fn window_excluded(window_name: &str, patterns: &[String]) -> bool {
    name_excluded(window_name, patterns)
}

/// True if `name` matches any blocklist pattern.
fn name_excluded(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_match(pattern, name))
}

/// Anchored glob match supporting `*` (any run, including empty) and `?`
/// (exactly one char). Works over `char`s so multi-byte session names
/// match correctly. Iterative backtracking; no recursion depth risk.
#[allow(dead_code)]
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_p: Option<usize> = None;
    let mut star_t = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_is_anchored_to_the_whole_name() {
        assert!(glob_match("popup", "popup"));
        assert!(!glob_match("popup", "popups"));
        assert!(!glob_match("pop", "popup"));
    }

    #[test]
    fn glob_star_matches_any_run_including_empty() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*_popup_*", "feat_popup_1"));
        assert!(glob_match("*_popup_*", "_popup_"));
        assert!(!glob_match("*_popup_*", "mypopup"));
    }

    #[test]
    fn glob_question_matches_exactly_one_char() {
        assert!(glob_match("scratch-?", "scratch-1"));
        assert!(!glob_match("scratch-?", "scratch-12"));
        assert!(!glob_match("scratch-?", "scratch-"));
    }

    #[test]
    fn session_excluded_matches_any_pattern() {
        let patterns = vec!["*_popup_*".to_string(), "scratch".to_string()];
        assert!(session_excluded("feat_popup_2", &patterns));
        assert!(session_excluded("scratch", &patterns));
        assert!(!session_excluded("main", &patterns));
    }

    #[test]
    fn session_excluded_is_false_for_empty_patterns() {
        assert!(!session_excluded("anything", &[]));
    }

    #[test]
    fn window_excluded_matches_any_pattern() {
        let patterns = vec!["*_popup_*".to_string(), "logs".to_string()];
        assert!(window_excluded("feat_popup_2", &patterns));
        assert!(window_excluded("logs", &patterns));
        assert!(!window_excluded("editor", &patterns));
    }

    #[test]
    fn window_excluded_is_false_for_empty_patterns() {
        assert!(!window_excluded("anything", &[]));
    }
}
