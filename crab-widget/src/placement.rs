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

use crab_common::{
    desktop,
    gui::{PERMISSION_MAX_HEIGHT, PromptSpot},
};
use snafu::Snafu;

const GAP: i32 = 8;
const MARGIN: i32 = 8;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PlacementError {
    #[snafu(display("No window placement for this desktop"))]
    Unsupported,
    #[cfg(feature = "hyprland")]
    #[snafu(display("Unable to talk to Hyprland at {path:?}"))]
    Socket {
        source: std::io::Error,
        path: std::path::PathBuf,
    },
    #[cfg(feature = "hyprland")]
    #[snafu(display("Hyprland rejected the window rule: {reply}"))]
    Rejected { reply: String },
    #[cfg(feature = "hyprland")]
    #[snafu(context(false))]
    Dir {
        source: crab_common::dirs::CommonError,
    },
    #[cfg(feature = "kde")]
    #[snafu(context(false))]
    KWin {
        source: crate::backend::kwin::KWinError,
    },
    #[cfg(feature = "x11")]
    #[snafu(context(false))]
    X11 {
        source: crate::backend::x11::X11Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub output_name: Option<String>,
    pub output: Area,
    pub crab: Area,
}

impl Location {
    #[must_use]
    pub fn center(&self) -> (f64, f64) {
        (
            f64::from(self.crab.x) + f64::from(self.crab.width) / 2.0,
            f64::from(self.crab.y) + f64::from(self.crab.height) / 2.0,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub side: Side,
    pub edge: i32,
    pub middle: i32,
}

#[must_use]
pub fn spot_of(location: &Location) -> Spot {
    let Location { output, crab, .. } = location;
    let (crab_x, crab_y) = (crab.x - output.x, crab.y - output.y);
    let (side, edge) = if 2 * crab_x + crab.width > output.width {
        (Side::Left, crab_x - GAP)
    } else {
        (Side::Right, crab_x + crab.width + GAP)
    };
    let half = i32::from(PERMISSION_MAX_HEIGHT) / 2 + MARGIN;
    Spot {
        side,
        edge,
        middle: (crab_y + crab.height / 2)
            .min(output.height - half)
            .max(half),
    }
}

#[must_use]
pub fn global_spot(location: &Location) -> PromptSpot {
    let Spot { side, edge, middle } = spot_of(location);
    PromptSpot {
        left: side == Side::Left,
        edge: location.output.x + edge,
        middle: location.output.y + middle,
    }
}

pub trait Placer {
    fn place(&mut self, location: &Location) -> Result<Option<PromptSpot>, PlacementError>;
}

pub fn open() -> Result<Box<dyn Placer>, PlacementError> {
    match desktop::detect() {
        #[cfg(feature = "hyprland")]
        Some(desktop::Session::Wayland(Some(desktop::Compositor::Hyprland))) => {
            Ok(Box::new(crate::backend::hyprland::Placer::open()?))
        }
        #[cfg(feature = "kde")]
        Some(desktop::Session::Wayland(Some(desktop::Compositor::KWin))) => {
            Ok(Box::new(crate::backend::kwin::Placer::open()?))
        }
        #[cfg(feature = "x11")]
        Some(desktop::Session::X11) => Ok(Box::new(crate::backend::x11::Placer::open()?)),
        _ => UnsupportedSnafu.fail(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[must_use]
    pub fn location(crab_x: i32, crab_y: i32) -> Location {
        Location {
            output_name: Some("eDP-2".to_owned()),
            output: Area {
                x: 2560,
                y: 360,
                width: 1920,
                height: 1080,
            },
            crab: Area {
                x: 2560 + crab_x,
                y: 360 + crab_y,
                width: 200,
                height: 200,
            },
        }
    }

    #[test]
    fn crab_on_the_right_puts_the_prompt_to_its_left() {
        assert_eq!(
            spot_of(&location(1720, 440)),
            Spot {
                side: Side::Left,
                edge: 1712,
                middle: 540,
            }
        );
    }

    #[test]
    fn crab_on_the_left_puts_the_prompt_to_its_right() {
        assert_eq!(
            spot_of(&location(-50, 100)),
            Spot {
                side: Side::Right,
                edge: 158,
                middle: 312,
            }
        );
    }

    #[test]
    fn tallest_prompt_stays_on_the_output() {
        assert_eq!(spot_of(&location(1720, 200)).middle, 304 + MARGIN);
        assert_eq!(spot_of(&location(1720, 880)).middle, 1080 - 304 - MARGIN);
    }

    #[test]
    fn global_spot_uses_global_coordinates() {
        assert_eq!(
            global_spot(&location(1720, 440)),
            PromptSpot {
                left: true,
                edge: 4272,
                middle: 900,
            }
        );
    }
}
