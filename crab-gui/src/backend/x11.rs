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

use snafu::{OptionExt, Snafu};
use x11rb::{
    CURRENT_TIME,
    connection::Connection,
    errors::{ConnectError, ConnectionError, ReplyError},
    protocol::xproto::{Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window},
    rust_connection::RustConnection,
};

use crate::{
    desktop::{Desktop, DesktopError},
    outputs::Output,
};

const PRIMARY: &str = "Primary";
const SECONDARY: &str = "Monitor";
const PAGER_SOURCE: u32 = 2;

const CLIENT_LIST: &str = "_NET_CLIENT_LIST";
const WM_PID: &str = "_NET_WM_PID";
const ACTIVE_WINDOW: &str = "_NET_ACTIVE_WINDOW";

#[derive(Debug, Snafu)]
pub enum X11Error {
    #[snafu(context(false), display("Unable to connect to the X server"))]
    Connect { source: ConnectError },
    #[snafu(context(false), display("X connection error"))]
    Connection { source: ConnectionError },
    #[snafu(context(false), display("X request failed"))]
    Reply { source: ReplyError },
    #[snafu(display("The X server has no screen {screen}"))]
    NoScreen { screen: usize },
    #[snafu(display("No window belongs to process {pid}"))]
    NoWindow { pid: u32 },
}

fn connect() -> Result<(RustConnection, Window), X11Error> {
    let (conn, screen) = RustConnection::connect(None)?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .context(NoScreenSnafu { screen })?
        .root;
    Ok((conn, root))
}

fn atom(conn: &RustConnection, name: &str) -> Result<Atom, X11Error> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

pub struct X11;

impl Desktop for X11 {
    fn focus(&self, pid: u32) -> Result<(), DesktopError> {
        Ok(focus(pid)?)
    }
}

fn focus(pid: u32) -> Result<(), X11Error> {
    let (conn, root) = connect()?;
    let (client_list, wm_pid, active_window) = (
        atom(&conn, CLIENT_LIST)?,
        atom(&conn, WM_PID)?,
        atom(&conn, ACTIVE_WINDOW)?,
    );
    let clients = conn
        .get_property(false, root, client_list, AtomEnum::WINDOW, 0, u32::MAX)?
        .reply()?;
    let windows: Vec<Window> = clients.value32().into_iter().flatten().collect();
    let pids = windows
        .iter()
        .map(|window| conn.get_property(false, *window, wm_pid, AtomEnum::CARDINAL, 0, 1))
        .collect::<Result<Vec<_>, _>>()?;
    let window = windows
        .into_iter()
        .zip(pids)
        .find_map(|(window, owner)| {
            owner
                .reply()
                .ok()?
                .value32()?
                .any(|owner| owner == pid)
                .then_some(window)
        })
        .context(NoWindowSnafu { pid })?;
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        ClientMessageEvent::new(
            32,
            window,
            active_window,
            [PAGER_SOURCE, CURRENT_TIME, 0, 0, 0],
        ),
    )?;
    conn.flush()?;
    Ok(())
}

pub fn outputs() -> Result<Vec<Output>, X11Error> {
    let (conn, root) = connect()?;
    Ok(crab_common::x11::monitors(&conn, root)?
        .into_iter()
        .map(|monitor| Output {
            name: monitor.name,
            description: if monitor.primary { PRIMARY } else { SECONDARY }.to_owned(),
            width: monitor.width as u32,
            height: monitor.height as u32,
        })
        .collect())
}
