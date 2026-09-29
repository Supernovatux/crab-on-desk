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
    env,
    io::{self, Read},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

use crab_common::{
    agent::{Agent, EventKind, MAX_MESSAGE_BYTES, PermissionDecision},
    dirs::{CommonError, get_sibling_executable},
    gui::WIDGET_BINARY,
    ipc::{self, IpcError},
};
use snafu::{OptionExt, ResultExt, Snafu};

mod claude;
mod process;

#[derive(Debug, Snafu)]
enum HookError {
    #[snafu(display("Usage: crab-hook <agent>"))]
    Usage,
    #[snafu(display("Unknown agent {name}"))]
    UnknownAgent {
        source: serde_plain::Error,
        name: String,
    },
    #[snafu(display("Unable to read the hook payload"))]
    Stdin { source: io::Error },
    #[snafu(context(false))]
    Claude { source: claude::ClaudeError },
    #[snafu(context(false))]
    Ipc { source: IpcError },
    #[snafu(display("Unable to receive the permission decision"))]
    Receive { source: io::Error },
    #[snafu(display("Invalid permission decision"))]
    Decision { source: serde_json::Error },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to start {WIDGET_BINARY}"))]
    Launch { source: io::Error },
}

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(3);
const LAUNCH_POLL: Duration = Duration::from_millis(50);

fn main() {
    if let Err(error) = run() {
        eprintln!("{}", snafu::Report::from_error(error));
    }
}

fn run() -> Result<(), HookError> {
    let name = env::args().nth(1).context(UsageSnafu)?;
    let agent: Agent = serde_plain::from_str(&name).context(UnknownAgentSnafu { name })?;
    let mut payload = Vec::new();
    io::stdin().read_to_end(&mut payload).context(StdinSnafu)?;
    let event = match agent {
        Agent::Claude => claude::parse(&payload)?,
    };
    let Some(event) = event else {
        return Ok(());
    };
    let Some(mut stream) = connect(&event.kind)? else {
        return Ok(());
    };
    ipc::send(&mut stream, &event)?;
    if matches!(event.kind, EventKind::PermissionRequest { .. })
        && let Some(decision) = receive_decision(stream)?
    {
        match agent {
            Agent::Claude => claude::print_decision(decision, io::stdout().lock())?,
        }
    }
    Ok(())
}

fn connect(kind: &EventKind) -> Result<Option<UnixStream>, HookError> {
    if let Some(stream) = ipc::connect()? {
        return Ok(Some(stream));
    }
    if !matches!(kind, EventKind::SessionStart { .. }) {
        return Ok(None);
    }
    let mut widget = crab_common::process::detached(&get_sibling_executable(WIDGET_BINARY)?)
        .spawn()
        .context(LaunchSnafu)?;
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    loop {
        thread::sleep(LAUNCH_POLL);
        let exited = widget.try_wait().context(LaunchSnafu)?.is_some();
        if let Some(stream) = ipc::connect()? {
            return Ok(Some(stream));
        }
        if exited || Instant::now() >= deadline {
            return Ok(None);
        }
    }
}

fn receive_decision(stream: UnixStream) -> Result<Option<PermissionDecision>, HookError> {
    let mut reply = Vec::new();
    stream
        .take(MAX_MESSAGE_BYTES)
        .read_to_end(&mut reply)
        .context(ReceiveSnafu)?;
    if reply.trim_ascii().is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&reply)
        .map(Some)
        .context(DecisionSnafu)
}
