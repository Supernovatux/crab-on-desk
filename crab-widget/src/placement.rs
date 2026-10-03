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
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crab_common::{
    desktop::{self, Compositor},
    dirs::{CommonError, get_hyprland_socket, get_kwin_placement_script},
    gui::{PERMISSION_APP_ID, PERMISSION_MAX_HEIGHT},
};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use zbus::interface;

use crate::kwin::{self, KWinError, Script};

const GAP: i32 = 8;
const MARGIN: i32 = 8;
const HYPRLAND_RULE: &str = "crab-on-desk-permission";
const KWIN_PLUGIN: &str = "crab-on-desk-placement";
const SPOT_PATH: &str = "/com/supernovatux/CrabOnDesk/Placement";
const SPOT_INTERFACE: &str = "com.supernovatux.CrabOnDesk.Placement";

#[derive(Debug, Snafu)]
pub enum PlacementError {
    #[snafu(display("No window placement for this desktop"))]
    Unsupported,
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to talk to Hyprland at {path:?}"))]
    Socket { source: io::Error, path: PathBuf },
    #[snafu(display("Hyprland rejected the window rule: {reply}"))]
    Rejected { reply: String },
    #[snafu(context(false))]
    KWin { source: KWinError },
    #[snafu(display("Unable to serve the placement spot"))]
    Serve { source: Box<zbus::Error> },
    #[snafu(display("The session bus gave no name"))]
    Unnamed,
    #[snafu(display("The placement spot lock is poisoned"))]
    Poisoned,
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
enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spot {
    side: Side,
    edge: i32,
    middle: i32,
}

fn spot_of(location: &Location) -> Spot {
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

pub trait Placer {
    fn place(&mut self, location: &Location) -> Result<(), PlacementError>;
}

pub fn open() -> Result<Box<dyn Placer>, PlacementError> {
    match desktop::detect() {
        Some(Compositor::Hyprland) => Ok(Box::new(Hyprland {
            socket: get_hyprland_socket()?,
        })),
        Some(Compositor::KWin) => Ok(Box::new(KWin::open()?)),
        None => UnsupportedSnafu.fail(),
    }
}

struct Hyprland {
    socket: PathBuf,
}

impl Hyprland {
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

fn hyprland_rule(spot: Option<(Spot, Option<&str>)>) -> String {
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
    format!(
        "hl.window_rule({{ name = {HYPRLAND_RULE:?}, match = {{ class = {class:?} }}, {effects} }})"
    )
}

impl Placer for Hyprland {
    fn place(&mut self, location: &Location) -> Result<(), PlacementError> {
        self.eval(&hyprland_rule(Some((
            spot_of(location),
            location.output_name.as_deref(),
        ))))
    }
}

impl Drop for Hyprland {
    fn drop(&mut self) {
        if let Err(error) = self.eval(&hyprland_rule(None)) {
            eprintln!("{}", snafu::Report::from_error(error));
        }
    }
}

type KWinSpot = (bool, i32, i32);

struct KWin {
    spot: Arc<Mutex<Option<KWinSpot>>>,
    _script: Script,
}

struct SpotServer {
    spot: Arc<Mutex<Option<KWinSpot>>>,
}

#[interface(name = "com.supernovatux.CrabOnDesk.Placement")]
impl SpotServer {
    #[zbus(out_args("left", "edge", "middle"))]
    fn spot(&self) -> zbus::fdo::Result<KWinSpot> {
        self.spot
            .lock()
            .ok()
            .and_then(|spot| *spot)
            .ok_or_else(|| zbus::fdo::Error::Failed("The crab has no position yet".to_owned()))
    }
}

impl KWin {
    fn open() -> Result<Self, PlacementError> {
        let spot = Arc::new(Mutex::new(None));
        let connection = kwin::connect()?;
        connection
            .object_server()
            .at(
                SPOT_PATH,
                SpotServer {
                    spot: Arc::clone(&spot),
                },
            )
            .map_err(Box::new)
            .context(ServeSnafu)?;
        let destination = connection
            .unique_name()
            .map(ToString::to_string)
            .context(UnnamedSnafu)?;
        let script = Script::load(
            &connection,
            KWIN_PLUGIN,
            get_kwin_placement_script()?,
            &kwin_script(&destination),
        )?;
        Ok(Self {
            spot,
            _script: script,
        })
    }
}

fn kwin_spot(spot: Spot, output: Area) -> KWinSpot {
    let Spot { side, edge, middle } = spot;
    (side == Side::Left, output.x + edge, output.y + middle)
}

fn kwin_script(destination: &str) -> String {
    format!(
        "function movePrompt(window, left, edge, middle) {{
  const current = window.frameGeometry;
  const x = left ? edge - current.width : edge;
  window.frameGeometry = {{ x: x, y: middle - current.height / 2, width: current.width, height: current.height }};
}}
function keepCentred(window) {{
  let height = window.frameGeometry.height;
  window.frameGeometryChanged.connect(() => {{
    const current = window.frameGeometry;
    if (Math.abs(current.height - height) < 1) return;
    const middle = current.y + height / 2;
    height = current.height;
    window.frameGeometry = {{ x: current.x, y: middle - current.height / 2, width: current.width, height: current.height }};
  }});
}}
workspace.windowAdded.connect((window) => {{
  if (window.resourceClass !== {PERMISSION_APP_ID:?}) return;
  callDBus({destination:?}, {SPOT_PATH:?}, {SPOT_INTERFACE:?}, \"Spot\", (left, edge, middle) => {{
    movePrompt(window, left, edge, middle);
    keepCentred(window);
  }});
}});
"
    )
}

impl Placer for KWin {
    fn place(&mut self, location: &Location) -> Result<(), PlacementError> {
        let spot = kwin_spot(spot_of(location), location.output);
        *self.spot.lock().map_err(|_| PlacementError::Poisoned)? = Some(spot);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(crab_x: i32, crab_y: i32) -> Location {
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
    fn hyprland_rule_targets_the_crab_output() {
        assert_eq!(
            hyprland_rule(Some((spot_of(&location(1720, 440)), Some("eDP-2")))),
            r#"hl.window_rule({ name = "crab-on-desk-permission", match = { class = "^com\\.supernovatux\\.CrabOnDesk\\.Permission$" }, monitor = "eDP-2", move = "1712-window_w 540-window_h/2" })"#
        );
    }

    #[test]
    fn kwin_spot_uses_global_coordinates() {
        assert_eq!(
            kwin_spot(spot_of(&location(1720, 440)), location(0, 0).output),
            (true, 4272, 900)
        );
    }
}
