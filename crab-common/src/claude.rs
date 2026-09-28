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

use serde::{Deserialize, Serialize};

pub const CLAUDE_CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";
pub const CLAUDE_CONFIG_DIR: &str = ".claude";
pub const CLAUDE_SETTINGS_FILE: &str = "settings.json";
pub const CLAUDE_PROCESS: &str = "claude";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Stop,
    StopFailure,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    Notification,
    Elicitation,
    WorktreeCreate,
    PermissionRequest,
}

impl HookEvent {
    pub const ALL: [Self; 16] = [
        Self::SessionStart,
        Self::SessionEnd,
        Self::UserPromptSubmit,
        Self::PreToolUse,
        Self::PostToolUse,
        Self::PostToolUseFailure,
        Self::Stop,
        Self::StopFailure,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::PreCompact,
        Self::PostCompact,
        Self::Notification,
        Self::Elicitation,
        Self::WorktreeCreate,
        Self::PermissionRequest,
    ];

    #[must_use]
    pub const fn is_blocking(self) -> bool {
        matches!(self, Self::PermissionRequest)
    }
}
