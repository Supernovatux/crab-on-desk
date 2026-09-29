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

use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    time::Duration,
};

use calloop::RegistrationToken;
use crab_common::{
    agent::{Agent, AgentEvent, EventKind, MAX_MESSAGE_BYTES, PermissionDecision},
    dirs::{CommonError, get_sibling_executable},
    gui::{GUI_BINARY, PERMISSION_MODE},
};
use rustix::{io::Errno, net::sockopt::socket_peercred, process::Pid};
use snafu::{OptionExt, ResultExt, Snafu};

const REPLY_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Snafu)]
pub enum PermissionError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to start the permission prompt"))]
    Spawn { source: io::Error },
    #[snafu(display("The permission prompt has no {pipe}"))]
    Pipe { pipe: String },
    #[snafu(display("Unable to encode the permission request"))]
    Encode { source: serde_json::Error },
    #[snafu(display("Unable to send the permission request to the prompt"))]
    Request { source: io::Error },
    #[snafu(display("Unable to identify the hook process"))]
    Peer { source: Errno },
    #[snafu(display("Unable to read the permission prompt's answer"))]
    Answer { source: io::Error },
    #[snafu(display("Invalid permission decision"))]
    Decision { source: serde_json::Error },
    #[snafu(display("Unable to reply to the hook"))]
    Reply { source: io::Error },
}

pub struct Prompt {
    pub session: (Agent, String),
    pub watches: Vec<RegistrationToken>,
    hook: UnixStream,
    child: Child,
}

#[must_use]
pub const fn settles(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Stop
            | EventKind::StopFailure
            | EventKind::PromptSubmit
            | EventKind::SessionEnd
            | EventKind::SessionClear
            | EventKind::PermissionRequest { .. }
    )
}

impl Prompt {
    pub fn open(event: &AgentEvent, hook: UnixStream) -> Result<Self, PermissionError> {
        let request = serde_json::to_vec(event).context(EncodeSnafu)?;
        let child = Command::new(get_sibling_executable(GUI_BINARY)?)
            .arg(PERMISSION_MODE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context(SpawnSnafu)?;
        let mut prompt = Self {
            session: (event.agent, event.session_id.clone()),
            watches: Vec::new(),
            hook,
            child,
        };
        let mut stdin = prompt
            .child
            .stdin
            .take()
            .context(PipeSnafu { pipe: "stdin" })?;
        stdin.write_all(&request).context(RequestSnafu)?;
        Ok(prompt)
    }

    #[must_use]
    pub fn prompt_pid(&self) -> u32 {
        self.child.id()
    }

    pub fn hook_pid(&self) -> Result<u32, PermissionError> {
        let credentials = socket_peercred(&self.hook).context(PeerSnafu)?;
        Ok(Pid::as_raw(Some(credentials.pid)) as u32)
    }

    pub fn answer(mut self) -> Result<(), PermissionError> {
        let mut answer = Vec::new();
        self.child
            .stdout
            .take()
            .context(PipeSnafu { pipe: "stdout" })?
            .take(MAX_MESSAGE_BYTES)
            .read_to_end(&mut answer)
            .context(AnswerSnafu)?;
        if answer.trim_ascii().is_empty() {
            return Ok(());
        }
        let decision: PermissionDecision =
            serde_json::from_slice(&answer).context(DecisionSnafu)?;
        let mut line = serde_json::to_vec(&decision).context(EncodeSnafu)?;
        line.push(b'\n');
        self.hook
            .set_write_timeout(Some(REPLY_TIMEOUT))
            .context(ReplySnafu)?;
        self.hook.write_all(&line).context(ReplySnafu)
    }
}

impl Drop for Prompt {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill().and_then(|()| self.child.wait().map(drop)) {
            eprintln!("Unable to stop the permission prompt: {error}");
        }
    }
}
