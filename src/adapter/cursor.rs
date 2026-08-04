use serde_json::Value;

use crate::event::{AgentEvent, AgentEventKind, EventAdapter};
use crate::tmux::CURSOR_AGENT;
use crate::tool_name::CanonicalTool;

use super::{HookRegistration, json_str, json_value_or_null, optional_str};

pub struct CursorAdapter;

impl CursorAdapter {
    /// Cursor CLI hook wiring (`cursor.com/docs/hooks`). Trigger names use
    /// Cursor's camelCase convention; internal event names come from
    /// `AgentEventKind::external_name()`.
    pub const HOOK_REGISTRATIONS: &'static [HookRegistration] = &[
        HookRegistration {
            trigger: "sessionStart",
            matcher: None,
            kind: AgentEventKind::SessionStart,
        },
        HookRegistration {
            trigger: "beforeSubmitPrompt",
            matcher: None,
            kind: AgentEventKind::UserPromptSubmit,
        },
        HookRegistration {
            trigger: "postToolUse",
            matcher: None,
            kind: AgentEventKind::ActivityLog,
        },
        HookRegistration {
            trigger: "afterAgentResponse",
            matcher: None,
            kind: AgentEventKind::AfterAgentResponse,
        },
        HookRegistration {
            trigger: "stop",
            matcher: None,
            kind: AgentEventKind::Stop,
        },
        HookRegistration {
            trigger: "sessionEnd",
            matcher: None,
            kind: AgentEventKind::SessionEnd,
        },
    ];
}

fn workspace_root(input: &Value) -> String {
    input
        .get("workspace_roots")
        .and_then(|v| v.as_array())
        .and_then(|roots| roots.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn cursor_ctx_fields(input: &Value) -> (String, Option<String>) {
    (workspace_root(input), optional_str(input, "session_id"))
}

/// Cursor emits Claude-style PascalCase tool names with a few Cursor-specific
/// renames (`Shell`, `Task`).
fn normalize_tool_name(raw: &str) -> String {
    let canonical = match raw {
        "Shell" => CanonicalTool::Bash,
        "Task" => CanonicalTool::Agent,
        other => return other.to_string(),
    };
    canonical.as_str().to_string()
}

/// `postToolUse` carries results in a JSON-stringified `tool_output` field.
fn parse_tool_output(input: &Value) -> Value {
    let raw = json_str(input, "tool_output");
    if raw.is_empty() {
        return Value::Null;
    }
    serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string()))
}

impl EventAdapter for CursorAdapter {
    fn parse(&self, event_name: &str, input: &Value) -> Option<AgentEvent> {
        let (cwd, session_id) = cursor_ctx_fields(input);
        match event_name {
            "session-start" => Some(AgentEvent::SessionStart {
                agent: CURSOR_AGENT.into(),
                cwd,
                permission_mode: String::new(),
                source: String::new(),
                worktree: None,
                agent_id: None,
                session_id,
            }),
            "user-prompt-submit" => Some(AgentEvent::UserPromptSubmit {
                agent: CURSOR_AGENT.into(),
                cwd,
                permission_mode: String::new(),
                prompt: json_str(input, "prompt").into(),
                worktree: None,
                agent_id: None,
                session_id,
            }),
            "activity-log" => {
                let raw_name = json_str(input, "tool_name");
                if raw_name.is_empty() {
                    return None;
                }
                let tool_name = normalize_tool_name(raw_name);
                Some(AgentEvent::ActivityLog {
                    tool_name,
                    tool_input: json_value_or_null(input, "tool_input"),
                    tool_response: parse_tool_output(input),
                })
            }
            "after-agent-response" => {
                let text = json_str(input, "text");
                if text.is_empty() {
                    return None;
                }
                Some(AgentEvent::AfterAgentResponse {
                    agent: CURSOR_AGENT.into(),
                    cwd,
                    text: text.into(),
                    session_id,
                })
            }
            "stop" => {
                let status = json_str(input, "status");
                if status == "error" {
                    Some(AgentEvent::StopFailure {
                        agent: CURSOR_AGENT.into(),
                        cwd,
                        permission_mode: String::new(),
                        error: status.into(),
                        worktree: None,
                        agent_id: None,
                        session_id,
                    })
                } else {
                    Some(AgentEvent::Stop {
                        agent: CURSOR_AGENT.into(),
                        cwd,
                        permission_mode: String::new(),
                        last_message: String::new(),
                        response: None,
                        worktree: None,
                        agent_id: None,
                        session_id,
                    })
                }
            }
            "session-end" => Some(AgentEvent::SessionEnd {
                end_reason: json_str(input, "reason").into(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_registrations_match_parse_arms() {
        super::super::assert_table_drift_free("cursor", CursorAdapter::HOOK_REGISTRATIONS);
    }

    #[test]
    fn session_start_uses_workspace_root_as_cwd() {
        let event = CursorAdapter
            .parse(
                "session-start",
                &json!({
                    "session_id": "sess-1",
                    "workspace_roots": ["/tmp/project"]
                }),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: CURSOR_AGENT.into(),
                cwd: "/tmp/project".into(),
                permission_mode: "".into(),
                source: "".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-1".into()),
            }
        );
    }

    #[test]
    fn user_prompt_submit() {
        let event = CursorAdapter
            .parse(
                "user-prompt-submit",
                &json!({
                    "prompt": "hello",
                    "workspace_roots": ["/tmp"],
                    "session_id": "sess-2",
                }),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::UserPromptSubmit {
                agent: CURSOR_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                prompt: "hello".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-2".into()),
            }
        );
    }

    #[test]
    fn activity_log_normalizes_shell_and_parses_tool_output() {
        let event = CursorAdapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "Shell",
                    "tool_input": {"command": "echo hi"},
                    "tool_output": "{\"output\":\"hi\\n\",\"exitCode\":0}"
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                tool_response,
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "echo hi");
                assert_eq!(tool_response["exitCode"], 0);
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn after_agent_response_requires_text() {
        assert!(
            CursorAdapter
                .parse("after-agent-response", &json!({}))
                .is_none()
        );
        let event = CursorAdapter
            .parse(
                "after-agent-response",
                &json!({
                    "text": "done",
                    "workspace_roots": ["/tmp"],
                }),
            )
            .unwrap();
        match event {
            AgentEvent::AfterAgentResponse { text, cwd, .. } => {
                assert_eq!(text, "done");
                assert_eq!(cwd, "/tmp");
            }
            other => panic!("expected AfterAgentResponse, got {:?}", other),
        }
    }

    #[test]
    fn stop_completed_is_idle_stop() {
        let event = CursorAdapter
            .parse(
                "stop",
                &json!({"status": "completed", "workspace_roots": ["/tmp"]}),
            )
            .unwrap();
        match event {
            AgentEvent::Stop { last_message, .. } => assert!(last_message.is_empty()),
            other => panic!("expected Stop, got {:?}", other),
        }
    }

    #[test]
    fn stop_error_maps_to_stop_failure() {
        let event = CursorAdapter
            .parse(
                "stop",
                &json!({"status": "error", "workspace_roots": ["/tmp"]}),
            )
            .unwrap();
        match event {
            AgentEvent::StopFailure { error, .. } => assert_eq!(error, "error"),
            other => panic!("expected StopFailure, got {:?}", other),
        }
    }

    #[test]
    fn session_end_uses_reason_field() {
        let event = CursorAdapter
            .parse("session-end", &json!({"reason": "completed"}))
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionEnd {
                end_reason: "completed".into()
            }
        );
    }

    #[test]
    fn claude_only_events_not_supported() {
        assert!(CursorAdapter.parse("notification", &json!({})).is_none());
        assert!(CursorAdapter.parse("subagent-start", &json!({})).is_none());
    }
}
