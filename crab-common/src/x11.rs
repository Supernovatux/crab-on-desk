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

use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{
        randr::ConnectionExt as _,
        xproto::{ConnectionExt as _, Window},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn monitors(conn: &impl Connection, root: Window) -> Result<Vec<Monitor>, ReplyError> {
    let mut monitors = conn.randr_get_monitors(root, true)?.reply()?.monitors;
    monitors.sort_by_key(|monitor| !monitor.primary);
    let names = monitors
        .iter()
        .map(|monitor| conn.get_atom_name(monitor.name))
        .collect::<Result<Vec<_>, _>>()?;
    monitors
        .iter()
        .zip(names)
        .map(|(monitor, name)| {
            Ok(Monitor {
                name: String::from_utf8_lossy(&name.reply()?.name).into_owned(),
                primary: monitor.primary,
                x: monitor.x.into(),
                y: monitor.y.into(),
                width: monitor.width.into(),
                height: monitor.height.into(),
            })
        })
        .collect()
}
