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

use std::env;

pub const HYPRLAND_INSTANCE: &str = "HYPRLAND_INSTANCE_SIGNATURE";
pub const CURRENT_DESKTOP: &str = "XDG_CURRENT_DESKTOP";
pub const KDE_DESKTOP: &str = "KDE";
pub const KWIN_CURSOR_SCRIPT: &str = "crab-on-desk-cursor.js";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compositor {
    Hyprland,
    KWin,
}

#[must_use]
pub fn detect() -> Option<Compositor> {
    if env::var_os(HYPRLAND_INSTANCE).is_some() {
        return Some(Compositor::Hyprland);
    }
    env::var(CURRENT_DESKTOP)
        .is_ok_and(|desktops| desktops.split(':').any(|desktop| desktop == KDE_DESKTOP))
        .then_some(Compositor::KWin)
}
