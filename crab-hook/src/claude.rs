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
    agent::{
        Agent, AgentEvent, CompactTrigger, EventKind, MAX_TOOL_INPUT_BYTES, PermissionDecision,
    },
    claude::{CLAUDE_PROCESS, HookEvent},
};
use serde::{Deserialize, Serialize};
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
    tool_input: Option<serde_json::Value>,
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
                tool_input: self.tool_input_text(),
            },
        }
    }

    fn tool_input_text(&self) -> String {
        let mut text = self
            .tool_input
            .as_ref()
            .and_then(|input| serde_json::to_string_pretty(input).ok())
            .unwrap_or_default();
        text.truncate(text.floor_char_boundary(MAX_TOOL_INPUT_BYTES));
        text
    }

    fn spawns_subagent(&self) -> bool {
        self.tool_name
            .as_deref()
            .is_some_and(|tool| SUBAGENT_TOOLS.contains(&tool))
    }
}
