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
    fs::{self, Permissions},
    io::{self, ErrorKind, Read},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

use calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic};
use crab_common::{
    agent::MAX_MESSAGE_BYTES,
    control::Message,
    dirs::{CommonError, get_agent_socket},
};
use snafu::{ResultExt, Snafu};

const SOCKET_MODE: u32 = 0o600;
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Snafu)]
pub enum AgentError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Another crab-on-desk is listening on {path:?}"))]
    AlreadyRunning { path: PathBuf },
    #[snafu(display("Unable to listen on {path:?}"))]
    Bind { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to add the agent socket to the event loop"))]
    Register { source: calloop::Error },
    #[snafu(display("Unable to read an agent message"))]
    Read { source: io::Error },
    #[snafu(display("Invalid agent message"))]
    Parse { source: serde_json::Error },
}

pub struct AgentSocket {
    path: PathBuf,
}

impl Drop for AgentSocket {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            eprintln!("Unable to remove {}: {error}", self.path.display());
        }
    }
}

pub fn listen<D: 'static>(
    handle: &LoopHandle<'static, D>,
    mut on_message: impl FnMut(Message, UnixStream, &mut D) + 'static,
) -> Result<AgentSocket, AgentError> {
    let path = get_agent_socket()?;
    let listener = bind(&path)?;
    let socket = AgentSocket { path };
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            move |_, listener, data| loop {
                match listener.accept() {
                    Ok((stream, _)) => match receive(&stream) {
                        Ok(Some(message)) => on_message(message, stream, data),
                        Ok(None) => {}
                        Err(error) => eprintln!("{}", snafu::Report::from_error(error)),
                    },
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        return Ok(PostAction::Continue);
                    }
                    Err(error) => return Err(error),
                }
            },
        )
        .map_err(|e| e.error)
        .context(RegisterSnafu)?;
    Ok(socket)
}

fn bind(path: &Path) -> Result<UnixListener, AgentError> {
    if UnixStream::connect(path).is_ok() {
        return AlreadyRunningSnafu { path }.fail();
    }
    match fs::remove_file(path) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(error).context(BindSnafu { path });
        }
        _ => {}
    }
    let listener = UnixListener::bind(path).context(BindSnafu { path })?;
    fs::set_permissions(path, Permissions::from_mode(SOCKET_MODE)).context(BindSnafu { path })?;
    listener.set_nonblocking(true).context(BindSnafu { path })?;
    Ok(listener)
}

fn receive(mut stream: &UnixStream) -> Result<Option<Message>, AgentError> {
    stream
        .set_read_timeout(Some(RECEIVE_TIMEOUT))
        .context(ReadSnafu)?;
    let mut message = Vec::new();
    Read::by_ref(&mut stream)
        .take(MAX_MESSAGE_BYTES)
        .read_to_end(&mut message)
        .context(ReadSnafu)?;
    if message.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&message)
        .map(Some)
        .context(ParseSnafu)
}
