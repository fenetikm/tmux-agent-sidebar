# Focus Notification Target Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a third target to the existing `focus` command — `tmux-agent-sidebar focus notification` — that jumps tmux focus to the agent pane whose desktop notification fired most recently.

**Architecture:** The timestamps already exist in tmux as three per-pane options written by `desktop_notification::notify_if_allowed` (`@pane_os_notify_task_completed`, `_task_failed`, `_permission_required`), each encoded `timestamp|fingerprint`. The new path reads them with one `tmux list-panes -a -F` call per key, intersects the results with the candidate list that cycling already computes (`eligible_pane_ids`), and picks the highest timestamp. No new writes, no changes to the shared `pane_format()`, no dependency on the running TUI.

**Tech Stack:** Rust (edition 2024), std only. Tests are plain `#[cfg(test)] mod tests` unit tests over pure functions — no tmux server and no rendered frames.

**Spec:** `docs/superpowers/specs/2026-07-28-focus-notification-design.md`

## Global Constraints

- Rust edition 2024 (`Cargo.toml`); no new dependencies.
- Run `cargo fmt` before **every** commit — CI runs `cargo fmt --check` and will fail otherwise.
- Every task ends green on `cargo test`, `cargo clippy`, `cargo fmt --check`.
- The subcommand token is `notification` only. `last` is **not** accepted as an alias.
- Exact status-line strings, copied verbatim:
  - `agent-sidebar: no recent agent notification to focus`
  - `agent-sidebar: no recent agent notification in this session`
  - `agent-sidebar: already on the last notified pane`
- Exact usage string: `usage: tmux-agent-sidebar focus <next|prev|notification> [--scope <all|session>]`
- Do **not** add notify-stamp fields to `pane_format()` in `src/tmux/query.rs`. That format's 28 fields are kept in lock-step with hand-maintained index constants.
- **The stamp query issues one `list-panes` call per stamp key, with a two-field format.** A stamp value is itself `timestamp|fingerprint`, so a line carrying more than one stamp cannot be split back apart — see Task 2's rationale. Do not "optimise" this into a single call.
- Cycling behaviour (`next` / `prev`) must not change. `Direction`, `select_target_pane`, `eligible_pane_ids`, and `no_target_message` keep their current semantics.
- The stamp query format is deliberately *unquoted* — `#{pane_id}|#{<key>}`, not `#{q:pane_id}|#{q:<key>}`. `#{q:...}` escapes both `|` and `%`, which breaks the two-field split and the pane-id match; `src/tmux/query.rs` can use it only because it unescapes afterwards via `split_tmux_fields`, and this path does not.
- Documentation is English (project writing guideline).
- Keybinding examples use `'"#{@agent_sidebar_bin}" focus …'` — double quotes inside single quotes, matching README line 73.

---

### Task 1: Expose the notification stamp keys and timestamp parser

`src/desktop_notification.rs` owns the `timestamp|fingerprint` encoding but keeps both the option-key mapping and the parser private. The focus command needs read access. Widening this module's surface — rather than re-parsing `|` inside the CLI — keeps the encoding owned by the writer.

**Files:**
- Modify: `src/desktop_notification.rs` (add `DesktopNotificationKind::ALL` after the enum at lines 14-19; add `stamp_option_keys` and `stamp_timestamp` after the existing private `stamp_option_key` at lines 211-217; add tests to `mod tests`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `pub const DesktopNotificationKind::ALL: [DesktopNotificationKind; 3]`
  - `pub fn stamp_option_keys() -> [&'static str; 3]`
  - `pub fn stamp_timestamp(raw: &str) -> Option<u64>`

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests` braces in `src/desktop_notification.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib desktop_notification::tests::stamp_
```

Expected: FAIL to compile — `cannot find function stamp_option_keys in this scope` and `cannot find function stamp_timestamp in this scope`.

- [ ] **Step 3: Write the minimal implementation**

The enum has no `impl` block yet, so add one immediately after the `DesktopNotificationKind` declaration (after line 19):

```rust
impl DesktopNotificationKind {
    /// Every kind that writes a notification stamp. `focus notification`
    /// walks this list to find the newest stamp on a pane, so a new kind
    /// becomes visible to it automatically.
    pub const ALL: [Self; 3] = [
        Self::TaskCompleted,
        Self::TaskFailed,
        Self::PermissionRequired,
    ];
}
```

Then add these two public functions directly below the existing private `stamp_option_key` (after line 217). `stamp_option_key` stays private: callers get the whole set or nothing, so no one outside can map a kind to a key by hand and drift from the writer.

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib desktop_notification::tests::stamp_
```

Expected: PASS, 3 tests.

- [ ] **Step 5: Verify the whole suite and lints are green**

```bash
cargo fmt && cargo test && cargo clippy
```

Expected: all tests pass, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add src/desktop_notification.rs
git commit -m "feat(notify): expose notification stamp keys and timestamp parser"
```

---

### Task 2: Parse stamp query output into (pane_id, timestamp) pairs

Turn one query's raw output into data. Pure and independently testable; the tmux call itself lands in Task 5.

**Why one line carries exactly one stamp:** the query format is `#{q:pane_id}|#{q:<one key>}`, so a line looks like `%1|1700000123|1699999999:notification`. A pane id never contains `|`, and `normalize_fingerprint` (`src/desktop_notification.rs:231`) replaces every `|` in a fingerprint with a space. So splitting at the **first** `|` cleanly separates the pane id from a stamp value that still has its own separator intact for `stamp_timestamp` to find. Packing all three keys into one line would break this: after splitting, no field would carry a separator, and scanning for numeric-looking fields would mistake an all-digit fingerprint for a timestamp.

**Files:**
- Modify: `src/cli/focus.rs` (extend the imports at line 1; add `parse_stamp_lines` after `eligible_pane_ids` at line 86; add tests to `mod tests`)

**Interfaces:**
- Consumes: `crate::desktop_notification::stamp_timestamp` (Task 1).
- Produces: `fn parse_stamp_lines(raw: &str) -> Vec<(String, u64)>` — one entry per line that carries a usable stamp. Lines with no pane id or no usable stamp are omitted. Because Task 5 concatenates the results of three per-key queries, the same pane id may appear in more than one entry; deduplication is **not** this function's job.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `mod tests` in `src/cli/focus.rs`:

```rust
    #[test]
    fn parse_stamp_lines_reads_a_pane_id_and_its_stamp() {
        let raw = "%1|1700000300|1700000000:notification\n%2|1700000100|1700000000:stop\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%1".to_string(), 1_700_000_300), ("%2".to_string(), 1_700_000_100)]
        );
    }

    #[test]
    fn parse_stamp_lines_skips_panes_whose_option_is_unset() {
        // tmux emits an empty field for an option that was never set.
        let raw = "%1|\n%2|1700000900|1700000000:stop\n";
        assert_eq!(parse_stamp_lines(raw), vec![("%2".to_string(), 1_700_000_900)]);
    }

    #[test]
    fn parse_stamp_lines_skips_malformed_stamps() {
        let raw = "%1|nonsense\n%2|notanumber|fingerprint\n%3|1700000700|fp\n";
        assert_eq!(parse_stamp_lines(raw), vec![("%3".to_string(), 1_700_000_700)]);
    }

    #[test]
    fn parse_stamp_lines_ignores_blank_and_id_less_lines() {
        let raw = "\n|1700000500|fp\n%4|1700000700|fp\n";
        assert_eq!(parse_stamp_lines(raw), vec![("%4".to_string(), 1_700_000_700)]);
    }

    #[test]
    fn parse_stamp_lines_keeps_a_fingerprint_containing_a_colon() {
        // Fingerprints are run-scoped ("<run_id>:<suffix>") and free text
        // beyond that; only `|` is normalised away.
        let raw = "%1|1700000800|1700000000:Permission required: write\n";
        assert_eq!(parse_stamp_lines(raw), vec![("%1".to_string(), 1_700_000_800)]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib cli::focus::tests::parse_stamp_lines
```

Expected: FAIL to compile — `cannot find function parse_stamp_lines in this scope`.

- [ ] **Step 3: Write the minimal implementation**

Extend the imports at the top of `src/cli/focus.rs` (line 1 currently reads `use crate::tmux::SessionInfo;`):

```rust
use crate::desktop_notification;
use crate::tmux::SessionInfo;
```

Add after `eligible_pane_ids` (after line 86):

```rust
/// Parse one stamp query's output into `(pane_id, timestamp)` pairs.
///
/// Each line is `pane_id|<stamp value>` for a single stamp key, and a stamp
/// value is itself `timestamp|fingerprint`. Splitting at the *first* `|`
/// is therefore exact: a pane id never contains `|`, and
/// `normalize_fingerprint` strips `|` from fingerprints, so the remainder
/// is one whole stamp value with its own separator intact.
///
/// Lines without a pane id, and panes whose option is unset or corrupt,
/// are omitted rather than reported with a zero timestamp: a pane that has
/// never notified must never win the comparison in
/// [`select_last_notified_pane`].
fn parse_stamp_lines(raw: &str) -> Vec<(String, u64)> {
    raw.lines()
        .filter_map(|line| {
            let (pane_id, stamp) = line.split_once('|')?;
            let pane_id = pane_id.trim();
            if pane_id.is_empty() {
                return None;
            }
            Some((pane_id.to_string(), desktop_notification::stamp_timestamp(stamp)?))
        })
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib cli::focus::tests::parse_stamp_lines
```

Expected: PASS, 5 tests.

- [ ] **Step 5: Verify the whole suite and lints are green**

```bash
cargo fmt && cargo test && cargo clippy
```

- [ ] **Step 6: Commit**

```bash
git add src/cli/focus.rs
git commit -m "feat(focus): parse notification stamp query output"
```

---

### Task 3: Select the most recently notified pane

The selection rule, plus the two new status-line messages. Still pure — the tmux plumbing is Task 5.

**Files:**
- Modify: `src/cli/focus.rs` (add `select_last_notified_pane` after `parse_stamp_lines`; add the messages after `no_target_message` at lines 90-95; add tests to `mod tests`)

**Interfaces:**
- Consumes: `eligible_pane_ids` and `Scope` (existing), the `Vec<(String, u64)>` shape from Task 2.
- Produces:
  - `fn select_last_notified_pane(eligible: &[String], stamps: &[(String, u64)]) -> Option<String>` — `stamps` may contain several entries per pane; a pane's recency is the max over its own entries.
  - `fn no_notification_message(scope: Scope) -> &'static str`
  - `const ALREADY_ON_NOTIFIED_PANE: &str`

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `mod tests` in `src/cli/focus.rs`. The `ids` helper keeps the candidate lists readable:

```rust
    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn last_notified_picks_the_highest_timestamp() {
        let stamps = vec![
            ("%1".to_string(), 100),
            ("%2".to_string(), 300),
            ("%3".to_string(), 200),
        ];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2", "%3"]), &stamps),
            Some("%2".into())
        );
    }

    #[test]
    fn last_notified_takes_the_newest_of_several_entries_for_one_pane() {
        // The three per-key queries are concatenated, so a pane that has
        // fired more than one kind of notification appears more than once.
        let stamps = vec![
            ("%1".to_string(), 100),
            ("%2".to_string(), 200),
            ("%1".to_string(), 400),
            ("%2".to_string(), 300),
        ];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_ignores_stamps_for_panes_outside_the_candidate_list() {
        // %9 is newer but is not an agent pane (or is out of scope).
        let stamps = vec![("%1".to_string(), 100), ("%9".to_string(), 999)];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_returns_none_when_no_candidate_has_a_stamp() {
        let stamps = vec![("%9".to_string(), 999)];
        assert_eq!(select_last_notified_pane(&ids(&["%1", "%2"]), &stamps), None);
    }

    #[test]
    fn last_notified_returns_none_for_an_empty_candidate_list() {
        let stamps = vec![("%1".to_string(), 100)];
        assert_eq!(select_last_notified_pane(&[], &stamps), None);
    }

    #[test]
    fn last_notified_breaks_ties_in_enumeration_order() {
        let stamps = vec![("%2".to_string(), 500), ("%1".to_string(), 500)];
        // Candidate order, not stamp order, decides the winner.
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_can_resolve_to_the_active_pane() {
        // The active pane is not filtered out — cmd_focus reports that case.
        let stamps = vec![("%1".to_string(), 900), ("%2".to_string(), 100)];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn session_scope_prefers_an_older_notification_inside_the_active_session() {
        let sessions = vec![
            session("one", vec![pane("%1", true, PaneStatus::Idle, "one")]),
            session("two", vec![pane("%2", false, PaneStatus::Idle, "two")]),
        ];
        // %2 in session "two" is newer, but the cursor is in session "one".
        let stamps = vec![("%1".to_string(), 100), ("%2".to_string(), 999)];
        let eligible = eligible_pane_ids(&sessions, Some("one"), Scope::Session);

        assert_eq!(
            select_last_notified_pane(&eligible, &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn no_notification_message_names_the_scope() {
        assert_eq!(
            no_notification_message(Scope::All),
            "agent-sidebar: no recent agent notification to focus"
        );
        assert_eq!(
            no_notification_message(Scope::Session),
            "agent-sidebar: no recent agent notification in this session"
        );
    }

    #[test]
    fn already_on_notified_pane_message_is_distinct() {
        assert_eq!(
            ALREADY_ON_NOTIFIED_PANE,
            "agent-sidebar: already on the last notified pane"
        );
        assert_ne!(ALREADY_ON_NOTIFIED_PANE, no_notification_message(Scope::All));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib cli::focus
```

Expected: FAIL to compile — `cannot find function select_last_notified_pane`, `cannot find function no_notification_message`, `cannot find value ALREADY_ON_NOTIFIED_PANE`.

- [ ] **Step 3: Write the minimal implementation**

Add after `parse_stamp_lines` in `src/cli/focus.rs`:

```rust
/// The candidate pane whose newest notification stamp is the most recent,
/// or `None` when no candidate has ever notified.
///
/// `stamps` is the concatenation of one query per stamp key, so a pane may
/// appear several times — once per notification kind it has fired. A
/// pane's recency is the maximum over its own entries.
///
/// Unlike [`select_target_pane`], the active pane is deliberately *not*
/// filtered out. Whether the newest notification came from the pane the
/// user is already on is a meaningful distinction the caller reports on
/// rather than something to hide.
///
/// Ties resolve to the earlier pane in `eligible` (tmux enumeration
/// order). The strict `>` is what enforces that: `max_by_key` would keep
/// the *last* equal maximum instead.
fn select_last_notified_pane(eligible: &[String], stamps: &[(String, u64)]) -> Option<String> {
    let mut best: Option<(&String, u64)> = None;
    for pane_id in eligible {
        let Some(timestamp) = stamps
            .iter()
            .filter(|(id, _)| id == pane_id)
            .map(|(_, timestamp)| *timestamp)
            .max()
        else {
            continue;
        };
        if best.is_none_or(|(_, best_timestamp)| timestamp > best_timestamp) {
            best = Some((pane_id, timestamp));
        }
    }
    best.map(|(pane_id, _)| pane_id.clone())
}
```

Add after `no_target_message` (after line 95):

```rust
/// Status-line text when no candidate pane has ever fired a notification.
/// Mirrors [`no_target_message`]: a silent no-op is indistinguishable from
/// a broken binary.
fn no_notification_message(scope: Scope) -> &'static str {
    match scope {
        Scope::All => "agent-sidebar: no recent agent notification to focus",
        Scope::Session => "agent-sidebar: no recent agent notification in this session",
    }
}

/// Status-line text when the most recent notification came from the pane
/// the cursor is already on. Distinct from [`no_notification_message`] so
/// the user can tell "nothing has notified" from "you are already there".
const ALREADY_ON_NOTIFIED_PANE: &str = "agent-sidebar: already on the last notified pane";
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib cli::focus
```

Expected: PASS — the 10 new tests plus every pre-existing focus test.

- [ ] **Step 5: Verify the whole suite and lints are green**

```bash
cargo fmt && cargo test && cargo clippy
```

- [ ] **Step 6: Commit**

```bash
git add src/cli/focus.rs
git commit -m "feat(focus): select the most recently notified pane"
```

---

### Task 4: Accept `notification` as a focus target

Replace the parsed `Direction` with a `Target` enum so the argument parser admits a third target. Cycling semantics are untouched; this is a widening of the parse result.

**Files:**
- Modify: `src/cli/focus.rs` (add the `Target` enum after `Direction` at lines 3-7; replace `parse_direction` at lines 101-107 with `parse_target`; change `parse_args` at lines 109-133; update `usage` at lines 97-99; rewrite the two existing `parse_args` tests at lines 401-415; add tests)

**Interfaces:**
- Consumes: `Direction`, `Scope` (existing), `no_notification_message` (Task 3, used by this task's temporary stub).
- Produces:
  - `enum Target { Cycle(Direction), Notification }` with `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`
  - `fn parse_args(args: &[String]) -> Result<(Target, Scope), ()>`

- [ ] **Step 1: Rewrite the existing tests and add the new ones**

The two existing `parse_args` tests assert on a bare `Direction`; leaving them would be a compile error rather than a red test. Replace lines 401-415 with:

```rust
    #[test]
    fn parse_args_defaults_to_all_scope() {
        assert_eq!(
            parse_args(&["next".into()]),
            Ok((Target::Cycle(Direction::Next), Scope::All))
        );
    }

    #[test]
    fn parse_args_accepts_session_scope_and_previous_alias() {
        assert_eq!(
            parse_args(&["previous".into(), "--scope".into(), "session".into()]),
            Ok((Target::Cycle(Direction::Prev), Scope::Session))
        );
    }
```

Then add:

```rust
    #[test]
    fn parse_args_accepts_the_notification_target() {
        assert_eq!(
            parse_args(&["notification".into()]),
            Ok((Target::Notification, Scope::All))
        );
        assert_eq!(
            parse_args(&["notification".into(), "--scope".into(), "session".into()]),
            Ok((Target::Notification, Scope::Session))
        );
    }

    #[test]
    fn parse_args_rejects_last_as_an_alias_for_notification() {
        assert_eq!(parse_args(&["last".into()]), Err(()));
    }

    #[test]
    fn parse_args_rejects_an_invalid_scope_for_the_notification_target() {
        assert_eq!(
            parse_args(&["notification".into(), "--scope".into(), "window".into()]),
            Err(())
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib cli::focus::tests::parse_args
```

Expected: FAIL to compile — `cannot find type Target in this scope`.

- [ ] **Step 3: Write the minimal implementation**

Add after the `Direction` enum (after line 7):

```rust
/// What the user asked `focus` to jump to. `Cycle` is the original
/// next/prev walk over the eligible pane list; `Notification` jumps
/// straight to the most recently notified pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Cycle(Direction),
    Notification,
}
```

Replace `parse_direction` (lines 101-107) with:

```rust
fn parse_target(value: &str) -> Option<Target> {
    match value {
        "next" => Some(Target::Cycle(Direction::Next)),
        "prev" | "previous" => Some(Target::Cycle(Direction::Prev)),
        "notification" => Some(Target::Notification),
        _ => None,
    }
}
```

Change `parse_args`' signature and first binding; the `--scope` loop body is unchanged:

```rust
fn parse_args(args: &[String]) -> Result<(Target, Scope), ()> {
    let target = args
        .first()
        .and_then(|value| parse_target(value))
        .ok_or(())?;
    let mut scope = Scope::All;
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
                let value = args.get(index + 1).ok_or(())?;
                scope = match value.as_str() {
                    "all" => Scope::All,
                    "session" => Scope::Session,
                    _ => return Err(()),
                };
                index += 2;
            }
            _ => return Err(()),
        }
    }

    Ok((target, scope))
}
```

Update `usage` (line 98):

```rust
fn usage() {
    eprintln!("usage: tmux-agent-sidebar focus <next|prev|notification> [--scope <all|session>]");
}
```

`cmd_focus` now destructures a `Target` where it expects a `Direction`, so it will not compile. Replace its `let (direction, scope) = match parse_args(args) { … };` block (lines 142-148) with a real dispatch whose `Notification` arm is an honest placeholder — Task 5 fills it in. Everything below that block stays exactly as it is:

```rust
    let (target, scope) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            usage();
            return 1;
        }
    };
    let direction = match target {
        Target::Cycle(direction) => direction,
        // Wired to tmux in the next commit. Parsing lands first so the
        // argument surface and its tests are reviewable on their own.
        Target::Notification => {
            crate::tmux::show_message(no_notification_message(scope));
            return 0;
        }
    };
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib cli::focus
```

Expected: PASS — 5 `parse_args` tests plus every other focus test.

- [ ] **Step 5: Verify the whole suite and lints are green**

```bash
cargo fmt && cargo test && cargo clippy
```

- [ ] **Step 6: Commit**

```bash
git add src/cli/focus.rs
git commit -m "feat(focus): accept notification as a focus target"
```

---

### Task 5: Wire the notification target to tmux

Replace Task 4's placeholder with the real path: query the stamps, select, jump. This is the one task whose deliverable can't be fully unit-tested — it is the tmux subprocess boundary — so it ends with a manual check in a live tmux session.

**Files:**
- Modify: `src/cli/focus.rs` (add `stamp_format`, `notification_stamps`, and `focus_last_notification` after `select_last_notified_pane`; replace the `Target::Notification` placeholder in `cmd_focus`; add a test)

**Interfaces:**
- Consumes: `desktop_notification::stamp_option_keys` (Task 1), `parse_stamp_lines` (Task 2), `select_last_notified_pane` / `no_notification_message` / `ALREADY_ON_NOTIFIED_PANE` (Task 3), `Target` (Task 4), plus the existing `eligible_pane_ids`, `crate::tmux::run_tmux`, `crate::tmux::show_message`, `crate::tmux::select_pane`.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Write the failing test for the format builder**

The subprocess call and the jump aren't unit-testable, but the format string is. Add to `mod tests` in `src/cli/focus.rs`:

```rust
    #[test]
    fn stamp_format_pairs_the_pane_id_with_one_quoted_key() {
        assert_eq!(
            stamp_format("@pane_os_notify_task_completed"),
            "#{q:pane_id}|#{q:@pane_os_notify_task_completed}"
        );
    }

    #[test]
    fn stamp_format_is_built_for_every_exposed_stamp_key() {
        // Guards against the query drifting from the notification kinds:
        // a new kind must show up here without touching this module.
        let formats: Vec<String> = desktop_notification::stamp_option_keys()
            .iter()
            .map(|key| stamp_format(key))
            .collect();
        assert_eq!(formats.len(), 3);
        for format in &formats {
            assert!(format.starts_with("#{q:pane_id}|#{q:@pane_os_notify_"));
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib cli::focus::tests::stamp_format
```

Expected: FAIL to compile — `cannot find function stamp_format in this scope`.

- [ ] **Step 3: Write the implementation**

Add after `select_last_notified_pane` in `src/cli/focus.rs`:

```rust
/// The `list-panes -F` format for one stamp key: the pane id paired with
/// that key's value.
///
/// One key per query, because a stamp value is itself
/// `timestamp|fingerprint` — see [`parse_stamp_lines`] for why a line
/// carrying several stamps cannot be split back apart.
///
/// These keys are deliberately absent from `tmux::query_sessions`' shared
/// `pane_format()`: its 28 fields are kept in lock-step with
/// hand-maintained index constants and the TUI has no use for notify
/// stamps, so `focus notification` pays for its own queries instead of
/// imposing maintenance cost on every consumer.
fn stamp_format(key: &str) -> String {
    format!("#{{q:pane_id}}|#{{q:{key}}}")
}

/// Every pane's notification stamps, one query per stamp key. A pane that
/// has fired several kinds of notification contributes one entry per kind;
/// [`select_last_notified_pane`] reduces those to the newest. A failed
/// query yields no entries, which reads the same as "nothing notified".
fn notification_stamps() -> Vec<(String, u64)> {
    desktop_notification::stamp_option_keys()
        .iter()
        .flat_map(|key| {
            let format = stamp_format(key);
            let raw = crate::tmux::run_tmux(&["list-panes", "-a", "-F", &format]).unwrap_or_default();
            parse_stamp_lines(&raw)
        })
        .collect()
}

/// Jump to the pane whose notification fired most recently, reporting on
/// the tmux status line when there is nowhere to go.
fn focus_last_notification(
    sessions: &[SessionInfo],
    active_pane_id: &str,
    active_session: Option<&str>,
    scope: Scope,
) -> i32 {
    let eligible = eligible_pane_ids(sessions, active_session, scope);
    let stamps = notification_stamps();

    match select_last_notified_pane(&eligible, &stamps) {
        None => crate::tmux::show_message(no_notification_message(scope)),
        Some(pane_id) if pane_id == active_pane_id => {
            crate::tmux::show_message(ALREADY_ON_NOTIFIED_PANE)
        }
        Some(pane_id) => crate::tmux::select_pane(&pane_id),
    }
    0
}
```

Then restructure `cmd_focus` so both targets share the active-pane resolution. Replace the whole function with:

```rust
pub fn cmd_focus(args: &[String]) -> i32 {
    let (target, scope) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            usage();
            return 1;
        }
    };

    let sessions = crate::tmux::query_sessions();
    let Some(active_pane_id) = active_pane_id() else {
        eprintln!("tmux-agent-sidebar focus: not running inside tmux");
        return 1;
    };
    let active_session = crate::tmux::pane_session_name(&active_pane_id);

    let direction = match target {
        Target::Cycle(direction) => direction,
        Target::Notification => {
            return focus_last_notification(
                &sessions,
                &active_pane_id,
                active_session.as_deref(),
                scope,
            );
        }
    };

    let Some(target_pane_id) = select_target_pane(
        &sessions,
        &active_pane_id,
        active_session.as_deref(),
        direction,
        scope,
    ) else {
        crate::tmux::show_message(no_target_message(scope));
        return 0;
    };

    crate::tmux::select_pane(&target_pane_id);
    0
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib cli::focus
```

Expected: PASS — every focus test including the two new `stamp_format` ones.

- [ ] **Step 5: Verify the whole suite and lints are green**

```bash
cargo fmt && cargo test && cargo clippy
```

- [ ] **Step 6: Build the release binary and verify by hand in tmux**

```bash
cargo build --release
```

`~/.tmux/plugins/tmux-agent-sidebar` is normally a symlink to this repo, so the release build is picked up directly. **If working in a git worktree**, copy and re-sign, or tmux will SIGKILL the binary (signal 9):

```bash
cp "$(pwd)/target/release/tmux-agent-sidebar" ~/.tmux/plugins/tmux-agent-sidebar/target/release/tmux-agent-sidebar
codesign --force --sign - ~/.tmux/plugins/tmux-agent-sidebar/target/release/tmux-agent-sidebar
```

Then, from inside tmux, check all four outcomes and record the actual output of each:

```bash
# 1. Usage error, exit 1 — `last` is not an alias
./target/release/tmux-agent-sidebar focus last ; echo "exit=$?"
# Expected: usage line naming <next|prev|notification>, exit=1

# 2. Reports rather than silently doing nothing when nothing has notified
./target/release/tmux-agent-sidebar focus notification ; echo "exit=$?"
# Expected on a server with no notifications yet: "agent-sidebar: no recent
# agent notification to focus" on the status line, exit=0

# 3. Jumps after a real notification: let an agent pane finish a task (or
#    request a permission), move the cursor to a different pane, then:
./target/release/tmux-agent-sidebar focus notification
# Expected: focus lands on the agent pane that notified, crossing session
# and window if needed

# 4. Already-on-target message: run it again without moving
./target/release/tmux-agent-sidebar focus notification
# Expected: "agent-sidebar: already on the last notified pane", exit=0

# 5. Cycling is unchanged
./target/release/tmux-agent-sidebar focus next ; echo "exit=$?"
# Expected: same behaviour as before this branch, exit=0
```

Confirm each outcome before proceeding. If step 3 reports "no recent agent notification" after a genuine notification, check whether desktop notifications are actually enabled:

```bash
tmux show -gv @sidebar_notifications
tmux show -gv @sidebar_notifications_events
```

A disabled or filtered event writes no stamp, and being invisible to `focus notification` is then intended behaviour, not a bug.

- [ ] **Step 7: Commit**

```bash
git add src/cli/focus.rs
git commit -m "feat(focus): jump to the most recently notified pane"
```

---

### Task 6: Document the notification target

Three user-facing docs describe the focus command's surface. All three must gain the new target or the feature is undiscoverable.

**Files:**
- Modify: `README.md:70-81`
- Modify: `website/src/content/docs/reference/keybindings.md:22-35`
- Modify: `website/src/content/docs/reference/scripting.md:46-71`

**Interfaces:**
- Consumes: the final command surface from Task 5.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Update README.md**

In the `### 3. Toggle the sidebar` section, append one line to the binding block (currently lines 73-76) so it reads:

```tmux
bind-key C-n run-shell '"#{@agent_sidebar_bin}" focus next --scope all'
bind-key C-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope all'
bind-key M-n run-shell '"#{@agent_sidebar_bin}" focus next --scope session'
bind-key M-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope session'
bind-key M-l run-shell '"#{@agent_sidebar_bin}" focus notification'
```

Then, immediately after the existing `--scope` paragraph (line 81), add:

```markdown
`focus notification` jumps to the pane whose desktop notification fired most recently, across every session unless you add `--scope session`. It follows your notification settings: an event that never produced a desktop notification is invisible to it.
```

- [ ] **Step 2: Update the keybindings reference**

In `website/src/content/docs/reference/keybindings.md`, add the same line to the block at lines 27-30:

```tmux
bind-key M-l run-shell '"#{@agent_sidebar_bin}" focus notification'
```

And after the paragraph at line 35, add:

```markdown
`focus notification` is a third target alongside `next` and `prev`. Instead of walking the list, it jumps straight to the pane whose desktop notification fired most recently — useful as a "take me to whatever just pinged me" key. It honours `--scope session` the same way the directions do.
```

- [ ] **Step 3: Update the scripting reference**

In `website/src/content/docs/reference/scripting.md`:

Replace line 50 with:

```markdown
Use `focus` from tmux bindings or scripts to jump between agent panes:
```

Replace the command surface at line 53 with:

```bash
tmux-agent-sidebar focus <next|prev|notification> [--scope <all|session>]
```

Add to the examples block after line 62:

```tmux
bind-key M-l run-shell '"#{@agent_sidebar_bin}" focus notification'
```

Add a new subsection immediately before `## Example status line snippet` (line 73):

```markdown
### The notification target

`focus notification` jumps to the agent pane whose desktop notification fired most recently, rather than walking the list. It reads the same `@pane_os_notify_task_completed`, `@pane_os_notify_task_failed`, and `@pane_os_notify_permission_required` pane options that the notification pipeline writes, so it works with the sidebar closed and survives a sidebar restart.

Because those options are only written when a desktop notification is actually delivered, the command follows your notification settings — an event suppressed by `@sidebar_notifications` or excluded from `@sidebar_notifications_events` leaves no trace for it to find. Repeat notifications with the same fingerprint inside the 120-second cooldown do not refresh the timestamp either.

When no eligible pane has ever notified, or when the most recent notification came from the pane you are already in, the command writes a note to the tmux status line and exits `0`.
```

- [ ] **Step 4: Verify**

```bash
cargo fmt --check && cargo test
```

The website is a separate Astro project. If `website/node_modules` exists, also run:

```bash
npm --prefix website run build
```

Expected: no errors. If `node_modules` is absent, skip it rather than installing dependencies — these edits are plain Markdown inside existing files and add no new links or components.

- [ ] **Step 5: Commit**

```bash
git add README.md website/src/content/docs/reference/keybindings.md website/src/content/docs/reference/scripting.md
git commit -m "docs: document the focus notification target"
```

---

## Verification

After Task 6, confirm the state of the whole branch:

```bash
cargo fmt --check && cargo clippy && cargo test
```

Report the actual test count and any failures rather than asserting success.

Spec requirements and where each is satisfied:

| Requirement | Task |
| --- | --- |
| `notification` token, no `last` alias | 4 |
| `--scope all\|session` honoured for the new target | 3 (session-filter test), 5 (wiring) |
| Recency from the three `@pane_os_notify_*` options, kinds ranked equally | 1, 2, 3 |
| One query per stamp key; `pane_format()` untouched | 5 |
| Stamp encoding stays owned by `desktop_notification` | 1 |
| Candidate list reuses `eligible_pane_ids` | 3, 5 |
| Ties resolve in enumeration order | 3 |
| Active pane not filtered out; distinct already-on-target message | 3, 5 |
| Malformed stamps skipped; never-notified panes omitted | 2 |
| Three exact status-line strings | 3 |
| Usage string updated | 4 |
| README, keybindings, scripting docs | 6 |
