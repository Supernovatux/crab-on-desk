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

pub const GUI_BINARY: &str = "crab-gui";
pub const WIDGET_BINARY: &str = "crab-widget";
pub const SETTINGS_MODE: &str = "settings";
pub const PERMISSION_MODE: &str = "perm";
pub const INIT_MODE: &str = "init";
pub const DISPLAYS_PAGE: &str = "displays";
pub const SETTINGS_LOCK: &str = "crab-on-desk-settings.lock";
pub const WIDGET_LOCK: &str = "crab-on-desk-widget.lock";
pub const SETTINGS_APP_ID: &str = "com.supernovatux.CrabOnDesk";
pub const PERMISSION_APP_ID: &str = "com.supernovatux.CrabOnDesk.Permission";
pub const PERMISSION_MAX_HEIGHT: u16 = 608;
pub const GENERATE_COMMAND: &str = "generate";
pub const INSTALL_HOOKS_COMMAND: &str = "install-hooks";
pub const UNINSTALL_HOOKS_COMMAND: &str = "uninstall-hooks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptSpot {
    pub left: bool,
    pub edge: i32,
    pub middle: i32,
}

impl PromptSpot {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";

    #[must_use]
    pub fn args(&self) -> [String; 3] {
        [
            if self.left { Self::LEFT } else { Self::RIGHT }.to_owned(),
            self.edge.to_string(),
            self.middle.to_string(),
        ]
    }

    #[must_use]
    pub fn parse(args: &[&str]) -> Option<Self> {
        let [side, edge, middle] = args else {
            return None;
        };
        let left = match *side {
            Self::LEFT => true,
            Self::RIGHT => false,
            _ => return None,
        };
        Some(Self {
            left,
            edge: edge.parse().ok()?,
            middle: middle.parse().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_spot_round_trips_through_args() {
        let spot = PromptSpot {
            left: true,
            edge: -12,
            middle: 540,
        };
        let args = spot.args();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(PromptSpot::parse(&args), Some(spot));
        assert_eq!(PromptSpot::parse(&["up", "1", "2"]), None);
        assert_eq!(PromptSpot::parse(&["left", "1"]), None);
    }
}
