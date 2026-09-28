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
    io::{self, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    time::Duration,
};

use serde::Serialize;
use snafu::{ResultExt, Snafu};

use crate::dirs::{CommonError, get_agent_socket};

const SEND_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Snafu)]
pub enum IpcError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to encode the message"))]
    Encode { source: serde_json::Error },
    #[snafu(display("Unable to send the message"))]
    Send { source: io::Error },
}

pub fn connect() -> Result<Option<UnixStream>, IpcError> {
    Ok(UnixStream::connect(get_agent_socket()?).ok())
}

pub fn send(stream: &mut UnixStream, message: &impl Serialize) -> Result<(), IpcError> {
    let mut line = serde_json::to_vec(message).context(EncodeSnafu)?;
    line.push(b'\n');
    stream
        .set_write_timeout(Some(SEND_TIMEOUT))
        .context(SendSnafu)?;
    stream.write_all(&line).context(SendSnafu)?;
    stream.shutdown(Shutdown::Write).context(SendSnafu)
}
