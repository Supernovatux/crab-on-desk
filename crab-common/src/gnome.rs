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

use std::sync::OnceLock;

use snafu::{ResultExt, Snafu};
use zbus::{blocking::Connection, proxy, zvariant::OwnedValue};

pub const EXTENSION_UUID: &str = "crab-on-desk@supernovatux";
pub const SHELL_SERVICE: &str = "org.gnome.Shell";
pub const EXTENSION_PATH: &str = "/com/supernovatux/CrabOnDesk";
pub const EXTENSION_INTERFACE: &str = "com.supernovatux.CrabOnDesk.Shell";

pub type Rect = (i32, i32, i32, i32);
pub type Monitor = (String, Rect);

#[derive(Debug, Snafu)]
pub enum GnomeError {
    #[snafu(display("D-Bus error while {thing}"))]
    Bus {
        source: Box<zbus::Error>,
        thing: String,
    },
    #[snafu(display("Unable to start {thing}"))]
    Spawn {
        source: std::io::Error,
        thing: String,
    },
    #[snafu(display(
        "The Crab on Desk GNOME Shell extension does not answer; install it and enable it ({EXTENSION_UUID}), then log out and in again"
    ))]
    Extension { source: Box<zbus::Error> },
}

#[proxy(
    interface = "com.supernovatux.CrabOnDesk.Shell",
    default_service = "org.gnome.Shell",
    default_path = "/com/supernovatux/CrabOnDesk"
)]
pub trait Shell {
    fn register(&self) -> zbus::Result<Monitor>;
    #[zbus(name = "Move")]
    fn move_window(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn monitors(&self) -> zbus::Result<Vec<Monitor>>;
    fn cursor_position(&self) -> zbus::Result<(i32, i32)>;
    fn set_prompt_spot(&self, left: bool, edge: i32, middle: i32) -> zbus::Result<()>;
    fn activate(&self, pid: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn moved(&self, connector: String, monitor: Rect, window: Rect) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.gnome.Shell.Extensions",
    default_service = "org.gnome.Shell",
    default_path = "/org/gnome/Shell"
)]
pub trait Extensions {
    fn get_extension_info(
        &self,
        uuid: &str,
    ) -> zbus::Result<std::collections::HashMap<String, OwnedValue>>;
    fn enable_extension(&self, uuid: &str) -> zbus::Result<bool>;
}

static CONNECTION: OnceLock<Connection> = OnceLock::new();

pub fn connection() -> Result<Connection, GnomeError> {
    if let Some(connection) = CONNECTION.get() {
        return Ok(connection.clone());
    }
    let connection = Connection::session().map_err(Box::new).context(BusSnafu {
        thing: "connecting to the session bus",
    })?;
    Ok(CONNECTION.get_or_init(|| connection).clone())
}

pub fn shell() -> Result<ShellProxyBlocking<'static>, GnomeError> {
    ShellProxyBlocking::new(&connection()?)
        .map_err(Box::new)
        .context(BusSnafu {
            thing: "creating the extension proxy",
        })
}

pub fn reply<T>(result: zbus::Result<T>, thing: &str) -> Result<T, GnomeError> {
    result.map_err(|error| {
        let missing = matches!(
            &error,
            zbus::Error::MethodError(name, _, _)
                if matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.UnknownObject"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                        | "org.freedesktop.DBus.Error.UnknownInterface"
                        | "org.freedesktop.DBus.Error.ServiceUnknown"
                )
        );
        if missing {
            GnomeError::Extension {
                source: Box::new(error),
            }
        } else {
            GnomeError::Bus {
                source: Box::new(error),
                thing: thing.to_owned(),
            }
        }
    })
}
