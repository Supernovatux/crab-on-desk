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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const AGENT_SOCKET: &str = "crab-on-desk.sock";
pub const HOOK_BINARY: &str = "crab-hook";
pub const MAX_MESSAGE_BYTES: u64 = 16 * 1024;
pub const MAX_TOOL_INPUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agent {
    Claude,
}

impl Agent {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    SessionStart { source: Option<String> },
    SessionEnd,
    SessionClear,
    PromptSubmit,
    ToolStart,
    ToolEnd,
    ToolFailure,
    Stop,
    StopFailure,
    SubagentStart { id: Option<String> },
    SubagentStop { id: Option<String> },
    CompactStart,
    CompactEnd { trigger: CompactTrigger },
    Notification,
    Elicitation,
    WorktreeCreate,
    PermissionRequest { tool_input: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompactTrigger {
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub agent: Agent,
    pub session_id: String,
    pub kind: EventKind,
    pub cwd: Option<PathBuf>,
    pub tool_name: Option<String>,
    pub source_pid: Option<u32>,
    pub agent_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "kebab-case")]
pub enum PermissionDecision {
    Allow,
    Deny { message: String },
}
