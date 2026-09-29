// crab-on-desk, a Rust based desktop pet for coding agents.
//     Copyright (C) 2026  Supernovatux thulashitharan.d@gmail.com
//
//     This program is free software: you can redistribute it and/or modify
//     it under the terms of the GNU Affero General Public License as
//     published by the Free Software Foundation, either version 3 of the
//     License, or (at your option) any later version.
//
//     This program is distributed in the hope that it will be useful,
//     but WITHOUT ANY WARRANTY; without even the implied warranty of
//     MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//     GNU Affero General Public License for more details.
//
//     You should have received a copy of the GNU Affero General Public License
//     along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{io::Write, path::PathBuf};

use crab_common::{
    agent::{Agent, AgentEvent, CompactTrigger, EventKind, PermissionDecision},
    claude::{CLAUDE_PROCESS, HookEvent},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use snafu::{ResultExt, Snafu};

use crate::process;

const DEFAULT_SESSION: &str = "default";
const CLEAR_REASON: &str = "clear";
const MANUAL_TRIGGER: &str = "manual";
const SUBAGENT_TOOLS: [&str; 2] = ["Task", "Agent"];

#[derive(Debug, Snafu)]
pub enum ClaudeError {
    #[snafu(display("Invalid Claude hook payload"))]
    Payload { source: serde_json::Error },
    #[snafu(display("Unable to write the permission decision"))]
    Decision { source: serde_json::Error },
}

#[derive(Deserialize)]
struct Payload {
    hook_event_name: String,
    session_id: Option<String>,
    cwd: Option<PathBuf>,
    tool_name: Option<String>,
    source: Option<String>,
    reason: Option<String>,
    trigger: Option<String>,
    agent_id: Option<String>,
    tool_input: Option<Value>,
    permission_suggestions: Option<Vec<Value>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DecisionOutput {
    hook_specific_output: PermissionOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionOutput {
    hook_event_name: HookEvent,
    decision: PermissionDecision,
}

pub fn parse(payload: &[u8]) -> Result<Option<AgentEvent>, ClaudeError> {
    let payload: Payload = serde_json::from_slice(payload).context(PayloadSnafu)?;
    let Ok(event) = serde_plain::from_str::<HookEvent>(&payload.hook_event_name) else {
        return Ok(None);
    };
    let kind = payload.kind(event);
    let pids = process::find(CLAUDE_PROCESS);
    Ok(Some(AgentEvent {
        agent: Agent::Claude,
        session_id: payload
            .session_id
            .unwrap_or_else(|| DEFAULT_SESSION.to_owned()),
        kind,
        cwd: payload.cwd,
        tool_name: payload.tool_name,
        source_pid: pids.source,
        agent_pid: pids.agent,
    }))
}

pub fn print_decision(decision: PermissionDecision, out: impl Write) -> Result<(), ClaudeError> {
    let output = DecisionOutput {
        hook_specific_output: PermissionOutput {
            hook_event_name: HookEvent::PermissionRequest,
            decision,
        },
    };
    serde_json::to_writer(out, &output).context(DecisionSnafu)
}

impl Payload {
    fn kind(&self, event: HookEvent) -> EventKind {
        match event {
            HookEvent::SessionStart => EventKind::SessionStart {
                source: self.source.clone(),
            },
            HookEvent::SessionEnd if self.reason.as_deref() == Some(CLEAR_REASON) => {
                EventKind::SessionClear
            }
            HookEvent::SessionEnd => EventKind::SessionEnd,
            HookEvent::UserPromptSubmit => EventKind::PromptSubmit,
            HookEvent::PreToolUse if self.spawns_subagent() => {
                EventKind::SubagentStart { id: None }
            }
            HookEvent::PreToolUse => EventKind::ToolStart,
            HookEvent::PostToolUse => EventKind::ToolEnd,
            HookEvent::PostToolUseFailure => EventKind::ToolFailure,
            HookEvent::Stop => EventKind::Stop,
            HookEvent::StopFailure => EventKind::StopFailure,
            HookEvent::SubagentStart => EventKind::SubagentStart {
                id: self.agent_id.clone(),
            },
            HookEvent::SubagentStop => EventKind::SubagentStop {
                id: self.agent_id.clone(),
            },
            HookEvent::PreCompact => EventKind::CompactStart,
            HookEvent::PostCompact => EventKind::CompactEnd {
                trigger: if self.trigger.as_deref() == Some(MANUAL_TRIGGER) {
                    CompactTrigger::Manual
                } else {
                    CompactTrigger::Auto
                },
            },
            HookEvent::Notification => EventKind::Notification,
            HookEvent::Elicitation => EventKind::Elicitation,
            HookEvent::WorktreeCreate => EventKind::WorktreeCreate,
            HookEvent::PermissionRequest => EventKind::PermissionRequest {
                tool_input: self.tool_input.clone().unwrap_or_default(),
                suggestions: self.permission_suggestions.clone().unwrap_or_default(),
            },
        }
    }

    fn spawns_subagent(&self) -> bool {
        self.tool_name
            .as_deref()
            .is_some_and(|tool| SUBAGENT_TOOLS.contains(&tool))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn printed(decision: PermissionDecision) -> Option<Value> {
        let mut out = Vec::new();
        print_decision(decision, &mut out).ok()?;
        serde_json::from_slice(&out).ok()
    }

    #[test]
    fn plain_allow_has_no_updates() {
        let decision = PermissionDecision::Allow {
            updated_input: None,
            updated_permissions: Vec::new(),
        };
        assert_eq!(
            printed(decision),
            Some(json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {"behavior": "allow"},
            }}))
        );
    }

    #[test]
    fn allow_carries_updates_in_claude_field_names() {
        let mode = json!({"type": "setMode", "mode": "auto", "destination": "session"});
        let decision = PermissionDecision::Allow {
            updated_input: Some(json!({"plan": "p"})),
            updated_permissions: vec![mode.clone()],
        };
        assert_eq!(
            printed(decision),
            Some(json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {
                    "behavior": "allow",
                    "updatedInput": {"plan": "p"},
                    "updatedPermissions": [mode],
                },
            }}))
        );
    }

    #[test]
    fn permission_request_keeps_input_and_suggestions() {
        let payload = json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "s",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "permission_suggestions": [{"type": "setMode", "mode": "auto"}],
        });
        assert_eq!(
            serde_json::from_value::<Payload>(payload)
                .ok()
                .map(|payload| payload.kind(HookEvent::PermissionRequest)),
            Some(EventKind::PermissionRequest {
                tool_input: json!({"command": "ls"}),
                suggestions: vec![json!({"type": "setMode", "mode": "auto"})],
            })
        );
    }
}
