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

#[cfg(not(any(feature = "wayland", feature = "x11")))]
compile_error!("enable at least one desktop feature: hyprland, kde or x11");

pub mod agent;
pub mod backend;
pub mod clicks;
pub mod cursor;
pub mod handler;
pub mod permission;
pub mod placement;
pub mod process;
pub mod random;
pub mod renderer;
pub mod spin;
pub mod state;
pub mod theme;
pub mod window;
