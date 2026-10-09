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
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use crab_common::{
    dirs::get_hyprland_socket,
    gui::{PERMISSION_APP_ID, PromptSpot},
};
use hyprland::{data::CursorPosition, shared::HyprData};
use snafu::{ResultExt, ensure};

use crate::{
    cursor::{CursorError, CursorSource, HyprlandSnafu, Position},
    placement::{self, Location, PlacementError, RejectedSnafu, Side, SocketSnafu, Spot},
};

const RULE: &str = "crab-on-desk-permission";

pub struct Cursor;

impl CursorSource for Cursor {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        let position = CursorPosition::get().context(HyprlandSnafu)?;
        Ok(Some((position.x as i32, position.y as i32)))
    }
}

pub struct Placer {
    socket: PathBuf,
}

impl Placer {
    pub fn open() -> Result<Self, PlacementError> {
        Ok(Self {
            socket: get_hyprland_socket()?,
        })
    }

    fn eval(&self, code: &str) -> Result<(), PlacementError> {
        let context = || SocketSnafu {
            path: self.socket.clone(),
        };
        let mut stream = UnixStream::connect(&self.socket).with_context(|_| context())?;
        stream
            .write_all(format!("/eval {code}").as_bytes())
            .with_context(|_| context())?;
        let mut reply = String::new();
        stream
            .read_to_string(&mut reply)
            .with_context(|_| context())?;
        ensure!(reply.trim() == "ok", RejectedSnafu { reply });
        Ok(())
    }
}

fn rule(spot: Option<(Spot, Option<&str>)>) -> String {
    let class = format!("^{}$", PERMISSION_APP_ID.replace('.', "\\."));
    let effects = spot.map_or_else(
        || "enabled = false".to_owned(),
        |(Spot { side, edge, middle }, output_name)| {
            let x = match side {
                Side::Left => format!("{edge}-window_w"),
                Side::Right => edge.to_string(),
            };
            let monitor =
                output_name.map_or_else(String::new, |name| format!("monitor = {name:?}, "));
            format!("{monitor}move = \"{x} {middle}-window_h/2\"")
        },
    );
    format!("hl.window_rule({{ name = {RULE:?}, match = {{ class = {class:?} }}, {effects} }})")
}

impl placement::Placer for Placer {
    fn place(&mut self, location: &Location) -> Result<Option<PromptSpot>, PlacementError> {
        self.eval(&rule(Some((
            placement::spot_of(location),
            location.output_name.as_deref(),
        ))))?;
        Ok(None)
    }
}

impl Drop for Placer {
    fn drop(&mut self) {
        if let Err(error) = self.eval(&rule(None)) {
            eprintln!("{}", snafu::Report::from_error(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::tests::location;

    #[test]
    fn rule_targets_the_crab_output() {
        assert_eq!(
            rule(Some((
                placement::spot_of(&location(1720, 440)),
                Some("eDP-2")
            ))),
            r#"hl.window_rule({ name = "crab-on-desk-permission", match = { class = "^com\\.supernovatux\\.CrabOnDesk\\.Permission$" }, monitor = "eDP-2", move = "1712-window_w 540-window_h/2" })"#
        );
    }
}
