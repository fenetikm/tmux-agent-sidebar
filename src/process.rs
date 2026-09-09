use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::tmux::AgentType;

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfo {
    pub(crate) comm: String,
    pub(crate) args: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessSnapshot {
    pub(crate) children_of: HashMap<u32, Vec<u32>>,
    pub(crate) info_by_pid: HashMap<u32, ProcessInfo>,
}

impl ProcessSnapshot {
    pub(crate) fn scan() -> Option<Self> {
        let output = Command::new("ps")
            .args(["-eo", "pid=,ppid=,comm=,args="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(Self::from_ps_output(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    pub(crate) fn from_ps_output(ps_output: &str) -> Self {
        let mut children_of: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut info_by_pid: HashMap<u32, ProcessInfo> = HashMap::new();

        for line in ps_output.lines() {
            let mut parts = line.split_whitespace();
            let Some(pid_str) = parts.next() else {
                continue;
            };
            let Some(ppid_str) = parts.next() else {
                continue;
            };
            let Ok(pid) = pid_str.parse::<u32>() else {
                continue;
            };
            let Ok(ppid) = ppid_str.parse::<u32>() else {
                continue;
            };
            let Some(comm) = parts.next() else {
                continue;
            };

            children_of.entry(ppid).or_default().push(pid);
            info_by_pid.insert(
                pid,
                ProcessInfo {
                    comm: comm.to_string(),
                    args: parts.collect::<Vec<_>>().join(" "),
                },
            );
        }

        Self {
            children_of,
            info_by_pid,
        }
    }

    pub(crate) fn descendants(&self, seed_pids: &[u32]) -> HashSet<u32> {
        let mut seen = HashSet::new();
        let mut queue: VecDeque<u32> = seed_pids.iter().copied().collect();

        while let Some(pid) = queue.pop_front() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some(children) = self.children_of.get(&pid) {
                for &child in children {
                    if !seen.contains(&child) {
                        queue.push_back(child);
                    }
                }
            }
        }

        seen
    }

    pub(crate) fn tree_has_agent(&self, seed_pids: &[u32], agent: &AgentType) -> bool {
        let agent_name = agent.as_str();
        self.descendants(seed_pids).into_iter().any(|pid| {
            self.info_by_pid
                .get(&pid)
                .map(|info| process_matches_agent(info, agent_name))
                .unwrap_or(false)
        })
    }

    pub(crate) fn command_lines_for_tree(&self, seed_pids: &[u32]) -> Vec<String> {
        self.descendants(seed_pids)
            .into_iter()
            .filter_map(|pid| self.info_by_pid.get(&pid))
            .map(|info| {
                if info.args.is_empty() {
                    info.comm.clone()
                } else {
                    info.args.trim().to_string()
                }
            })
            .collect()
    }
}

pub(crate) fn command_basename(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
}

pub(crate) fn process_matches_agent(info: &ProcessInfo, agent_name: &str) -> bool {
    if command_basename(&info.comm) == agent_name {
        return true;
    }

    let Some(command) = info.args.split_whitespace().next() else {
        return false;
    };
    command_basename(command.trim_matches('"')) == agent_name
}

/// How a deadline-bounded child ended.
///
/// Shared by the `gh pr view` lookup in `git.rs` and the custom panel
/// command, both of which must never let a hung child stall a polling
/// thread. Output is read on dedicated worker threads *and* the wait is a
/// `try_wait` poll loop on the calling thread, so a child that fills the
/// stdout pipe cannot deadlock the wait, and a hung wait cannot block the
/// readers either.
#[derive(Debug)]
pub enum RunOutcome {
    /// The child exited on its own. Inspect `status`, `stdout`, `stderr`.
    Completed(Output),
    /// The child outlived `timeout` and was killed.
    TimedOut,
    /// The child never started (missing binary, permissions, fork failure).
    // The message is surfaced by tests today and by a later panel-command
    // caller; `fetch_pr_number` currently discards it via `_ => None`.
    #[allow(dead_code)]
    SpawnFailed(String),
}

/// Run `cmd` to completion, killing it if it outlives `timeout`.
///
/// stdout and stderr are captured; stdin is closed so a child that reads
/// input fails fast instead of blocking on a terminal that isn't there.
///
/// The child is made the leader of its own process group (`process_group(0)`)
/// so that on timeout the *whole* group can be signalled, not just the
/// direct child. A `sh -c "slow | filter"` style command spawns
/// grandchildren that inherit the stdout pipe; killing only `sh` would leave
/// those grandchildren holding the pipe open, so the reader threads below
/// would never see EOF. The same applies to a backgrounded descendant that
/// outlives the direct child, e.g. `sh -c "slow-thing & printf hi"`: the
/// shell exits immediately, but the backgrounded process still holds the
/// pipe open, so even after `try_wait` reports the child gone, reading the
/// pipes to EOF must stay bounded by the deadline rather than block
/// unboundedly on that orphan.
pub fn run_with_deadline(cmd: &mut Command, timeout: Duration) -> RunOutcome {
    let mut child = match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return RunOutcome::SpawnFailed(err.to_string()),
    };

    // `process_group(0)` makes the child its own group leader, so its PID
    // doubles as its process group ID.
    let pid = child.id() as libc::pid_t;

    // Read stdout/stderr on their own threads so a child that fills a pipe
    // buffer cannot deadlock the poll loop below. `child` itself stays on
    // this thread (unlike the previous version, which moved it onto a
    // reader thread) so this thread can still `try_wait`/`wait` it and,
    // on timeout, kill and reap it directly. Each thread sends its buffer
    // over a channel (instead of returning it from the `JoinHandle`) so the
    // calling thread can wait for it with a bounded `recv_timeout` rather
    // than an unbounded `join`.
    let mut stdout_pipe = child.stdout.take().expect("stdout was piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr was piped");
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        let _ = stdout_tx.send(buf);
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        let _ = stderr_tx.send(buf);
    });

    let deadline = Instant::now() + timeout;
    let wait_result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(Some(status)),
            Ok(None) => {
                if Instant::now() >= deadline {
                    break Ok(None);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => break Err(err),
        }
    };

    match wait_result {
        Ok(Some(status)) => {
            // The direct child has exited, but a descendant that inherited
            // the stdout/stderr pipes (e.g. a backgrounded `&` job) can
            // still be holding them open, so `read_to_end` on the reader
            // threads may not have seen EOF yet. Wait for each reader only
            // until the *original* deadline, not a fresh timeout — this is
            // what keeps the deadline hard even on the success path.
            let stdout_result =
                stdout_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
            let stderr_result =
                stderr_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));

            match (stdout_result, stderr_result) {
                (Ok(stdout), Ok(stderr)) => {
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    RunOutcome::Completed(Output {
                        status,
                        stdout,
                        stderr,
                    })
                }
                _ => {
                    // A descendant outlived the direct child and is still
                    // holding a pipe open past the deadline. Force EOF by
                    // killing the whole process group, then the readers can
                    // finish and we join them.
                    //
                    // SAFETY: the direct child's pid was already reaped by
                    // `try_wait` above, but a process group ID stays live
                    // as long as it has members — and the orphaned
                    // descendant holding the pipe open is still a member of
                    // this one. POSIX/Linux will not hand `pid` out as a
                    // fresh PID to an unrelated process while it is still
                    // pinned as an active process group ID, so `kill(-pid,
                    // ...)` still reaches only processes descended from
                    // this call's child, not a recycled PID.
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    // Deliberately TimedOut, not Completed with whatever
                    // partial output we did capture: the next consumer
                    // parses NDJSON, and silently truncated output would
                    // render a plausible-looking short list instead of
                    // surfacing an error. A reported failure beats a wrong
                    // answer.
                    RunOutcome::TimedOut
                }
            }
        }
        Ok(None) => {
            // SAFETY: this thread — not a reader thread — has held `child`
            // unreaped since spawn, so the kernel cannot recycle `pid`
            // (nor, since `process_group(0)` made it the group leader, the
            // process group `pid` names) out from under us. `child.wait()`
            // just below is what finally releases it. `kill(-pid, ...)`
            // therefore reaches a process group this process still owns,
            // signalling every descendant that inherited the stdout/stderr
            // pipes rather than just the direct child.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            RunOutcome::TimedOut
        }
        Err(err) => {
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            RunOutcome::SpawnFailed(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    #[test]
    fn completed_returns_stdout_and_status() {
        let mut cmd = sh("printf 'hello'");
        match run_with_deadline(&mut cmd, Duration::from_secs(5)) {
            RunOutcome::Completed(out) => {
                assert!(out.status.success());
                assert_eq!(String::from_utf8_lossy(&out.stdout), "hello");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn completed_captures_failure_status_and_stderr() {
        let mut cmd = sh("printf 'boom' >&2; exit 3");
        match run_with_deadline(&mut cmd, Duration::from_secs(5)) {
            RunOutcome::Completed(out) => {
                assert_eq!(out.status.code(), Some(3));
                assert_eq!(String::from_utf8_lossy(&out.stderr), "boom");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn large_output_does_not_deadlock() {
        // ~200 KB, far past the pipe buffer. A try_wait polling loop that
        // never drains stdout would hang here until the deadline.
        let mut cmd = sh("yes abcdefghijklmnopqrstuvwxyz | head -n 8000");
        match run_with_deadline(&mut cmd, Duration::from_secs(10)) {
            RunOutcome::Completed(out) => assert_eq!(out.stdout.lines().count(), 8000),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn slow_child_times_out() {
        let mut cmd = sh("sleep 5");
        let started = std::time::Instant::now();
        match run_with_deadline(&mut cmd, Duration::from_millis(200)) {
            RunOutcome::TimedOut => {}
            other => panic!("expected TimedOut, got {other:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "should return at the deadline, not wait out the child"
        );
    }

    #[test]
    fn timeout_kills_whole_process_group_not_just_direct_child() {
        // `cat` inherits the write end of our stdout pipe from the shell
        // and only exits once `sleep` finishes (closing `cat`'s stdin) or
        // `cat` itself is signalled. Killing only the direct child (`sh`)
        // would leave `cat` holding the pipe open, so the reader thread
        // would never see EOF and this call would block for ~5s instead of
        // returning at the deadline.
        let mut cmd = sh("sleep 5 | cat");
        let started = std::time::Instant::now();
        match run_with_deadline(&mut cmd, Duration::from_millis(200)) {
            RunOutcome::TimedOut => {}
            other => panic!("expected TimedOut, got {other:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "should return at the deadline, not hang on a leaked descendant"
        );
    }

    #[test]
    fn backgrounded_descendant_holding_stdout_still_times_out_promptly() {
        // The shell exits immediately (backgrounding `sleep 5` and printing
        // "hi"), so `try_wait` reports the child gone almost instantly. But
        // `sleep 5` inherits the write end of the stdout pipe and keeps it
        // open for 5s. Before this fix, the post-exit `join()` was
        // unbounded, so this call would block for ~5s despite a 200ms
        // deadline — the deadline must stay hard even on this success-ish
        // path, so the correct outcome is TimedOut, not Completed with
        // partial/full output.
        let mut cmd = sh("sleep 5 & printf hi");
        let started = std::time::Instant::now();
        match run_with_deadline(&mut cmd, Duration::from_millis(200)) {
            RunOutcome::TimedOut => {}
            other => panic!("expected TimedOut, got {other:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "should return near the deadline, not wait out the orphaned descendant"
        );
    }

    #[test]
    fn missing_binary_reports_spawn_failure() {
        let mut cmd = Command::new("definitely-not-a-real-binary-xyz");
        match run_with_deadline(&mut cmd, Duration::from_secs(5)) {
            RunOutcome::SpawnFailed(msg) => assert!(!msg.is_empty()),
            other => panic!("expected SpawnFailed, got {other:?}"),
        }
    }

    #[test]
    fn descendants_walks_process_tree() {
        let snapshot = ProcessSnapshot {
            children_of: HashMap::from([(1, vec![2, 3]), (2, vec![4]), (4, vec![5])]),
            info_by_pid: HashMap::new(),
        };
        let seen = snapshot.descendants(&[1]);
        assert!(seen.contains(&1));
        assert!(seen.contains(&2));
        assert!(seen.contains(&3));
        assert!(seen.contains(&4));
        assert!(seen.contains(&5));
    }

    #[test]
    fn parse_ps_processes_preserves_spaced_args() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "100 1 codex /Applications/Codex App/bin/codex --full-auto\n101 100 sh sh -c wrapper\n",
        );

        assert_eq!(snapshot.children_of.get(&1).cloned(), Some(vec![100]));
        let info = snapshot.info_by_pid.get(&100).expect("process info");
        assert_eq!(info.comm, "codex");
        assert_eq!(info.args, "/Applications/Codex App/bin/codex --full-auto");
    }

    #[test]
    fn tree_has_agent_matches_descendant_process_name() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "100 1 fish fish -c opencode\n101 100 opencode opencode\n",
        );

        assert!(snapshot.tree_has_agent(&[100], &AgentType::OpenCode));
        assert!(!snapshot.tree_has_agent(&[100], &AgentType::Codex));
    }

    #[test]
    fn process_matches_agent_requires_command_name_match() {
        assert!(process_matches_agent(
            &ProcessInfo {
                comm: "claude".to_string(),
                args: "/opt/homebrew/bin/claude --flag".to_string(),
            },
            "claude",
        ));
        assert!(process_matches_agent(
            &ProcessInfo {
                comm: "node".to_string(),
                args: "/usr/local/bin/opencode".to_string(),
            },
            "opencode",
        ));
        assert!(!process_matches_agent(
            &ProcessInfo {
                comm: "not-opencode".to_string(),
                args: "/usr/local/bin/not-opencode".to_string(),
            },
            "opencode",
        ));
    }
}
