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

use crab_common::gui::PromptSpot;
use snafu::{OptionExt, Snafu};
use x11rb::{
    connection::Connection,
    errors::{ConnectError, ConnectionError, ReplyError, ReplyOrIdError},
    protocol::xproto::{ConnectionExt as _, Screen},
    resource_manager::{self, Database},
    xcb_ffi::XCBConnection,
};

use crate::{
    cursor::{CursorError, CursorSource, Position},
    placement::{self, Location, PlacementError},
};

const XFT_DPI: &str = "Xft.dpi";
const BASE_DPI: f64 = 96.0;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum X11Error {
    #[snafu(display("Unable to connect to the X server"))]
    #[snafu(context(false))]
    Connect { source: ConnectError },
    #[snafu(display("X connection error"))]
    #[snafu(context(false))]
    Connection { source: ConnectionError },
    #[snafu(display("X request failed"))]
    #[snafu(context(false))]
    Reply { source: ReplyError },
    #[snafu(display("X request failed"))]
    #[snafu(context(false))]
    ReplyOrId { source: ReplyOrIdError },
    #[snafu(display("The X server has no screen {screen}"))]
    NoScreen { screen: usize },
    #[snafu(display("The X server gave an invalid window id"))]
    InvalidWindow,
}

pub fn connect() -> Result<(XCBConnection, usize), X11Error> {
    Ok(XCBConnection::connect(None)?)
}

pub fn screen(conn: &impl Connection, screen: usize) -> Result<&Screen, X11Error> {
    conn.setup()
        .roots
        .get(screen)
        .context(NoScreenSnafu { screen })
}

pub fn resources(conn: &impl Connection) -> Result<Database, X11Error> {
    Ok(resource_manager::new_from_default(conn)?)
}

#[must_use]
pub fn scale(resources: &Database) -> f64 {
    resources
        .get_value::<u32>(XFT_DPI, "")
        .ok()
        .flatten()
        .map_or(1.0, |dpi| f64::from(dpi) / BASE_DPI)
}

pub struct Cursor {
    conn: XCBConnection,
    root: u32,
}

impl Cursor {
    pub fn open() -> Result<Self, X11Error> {
        let (conn, number) = connect()?;
        let root = screen(&conn, number)?.root;
        Ok(Self { conn, root })
    }
}

impl CursorSource for Cursor {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        let pointer = self
            .conn
            .query_pointer(self.root)
            .map_err(X11Error::from)?
            .reply()
            .map_err(X11Error::from)?;
        Ok(Some((pointer.root_x.into(), pointer.root_y.into())))
    }
}

pub struct Placer {
    scale: f64,
}

impl Placer {
    pub fn open() -> Result<Self, X11Error> {
        let (conn, _) = connect()?;
        Ok(Self {
            scale: scale(&resources(&conn)?),
        })
    }

    fn logical(&self, value: i32) -> i32 {
        (f64::from(value) / self.scale).round() as i32
    }
}

impl placement::Placer for Placer {
    fn place(&mut self, location: &Location) -> Result<Option<PromptSpot>, PlacementError> {
        let PromptSpot { left, edge, middle } = placement::global_spot(location);
        Ok(Some(PromptSpot {
            left,
            edge: self.logical(edge),
            middle: self.logical(middle),
        }))
    }
}
