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

use std::sync::{Arc, Mutex};

use crab_common::{
    desktop::{self, Compositor},
    dirs::{CommonError, get_kwin_cursor_script},
};
use hyprland::{data::CursorPosition, error::HyprError, shared::HyprData};
use snafu::{ResultExt, Snafu};
use zbus::interface;

use crate::kwin::{self, KWinError, Script};

const KWIN_PLUGIN: &str = "crab-on-desk-cursor";
const RECEIVER_PATH: &str = "/com/supernovatux/CrabOnDesk/Cursor";
const RECEIVER_INTERFACE: &str = "com.supernovatux.CrabOnDesk.Cursor";
const RECEIVER_METHOD: &str = "Moved";
const SAMPLE_INTERVAL_MS: u32 = 100;

pub type Position = (i32, i32);

#[derive(Debug, Snafu)]
pub enum CursorError {
    #[snafu(display("No cursor source for this desktop"))]
    Unsupported,
    #[snafu(display("Hyprland did not report the cursor"))]
    Hyprland { source: HyprError },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(context(false))]
    KWin { source: KWinError },
    #[snafu(display("Unable to serve the cursor receiver"))]
    Serve { source: Box<zbus::Error> },
    #[snafu(display("The KWin cursor receiver is gone"))]
    Receiver,
}

pub trait CursorSource {
    fn position(&mut self) -> Result<Option<Position>, CursorError>;
}

pub fn open() -> Result<Box<dyn CursorSource>, CursorError> {
    match desktop::detect() {
        Some(Compositor::Hyprland) => Ok(Box::new(Hyprland)),
        Some(Compositor::KWin) => Ok(Box::new(KWin::open()?)),
        None => UnsupportedSnafu.fail(),
    }
}

struct Hyprland;

impl CursorSource for Hyprland {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        let position = CursorPosition::get().context(HyprlandSnafu)?;
        Ok(Some((position.x as i32, position.y as i32)))
    }
}

struct KWin {
    latest: Arc<Mutex<Option<Position>>>,
    _script: Script,
}

struct Receiver {
    latest: Arc<Mutex<Option<Position>>>,
}

#[interface(name = "com.supernovatux.CrabOnDesk.Cursor")]
impl Receiver {
    fn moved(&self, x: i32, y: i32) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some((x, y));
        }
    }
}

impl KWin {
    fn open() -> Result<Self, CursorError> {
        let latest = Arc::new(Mutex::new(None));
        let connection = kwin::connect()?;
        connection
            .object_server()
            .at(
                RECEIVER_PATH,
                Receiver {
                    latest: Arc::clone(&latest),
                },
            )
            .map_err(Box::new)
            .context(ServeSnafu)?;
        let destination = connection
            .unique_name()
            .map(ToString::to_string)
            .ok_or(CursorError::Receiver)?;
        let script = Script::load(
            &connection,
            KWIN_PLUGIN,
            get_kwin_cursor_script()?,
            &sampler(&destination),
        )?;
        Ok(Self {
            latest,
            _script: script,
        })
    }
}

impl CursorSource for KWin {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        self.latest
            .lock()
            .map(|latest| *latest)
            .map_err(|_| CursorError::Receiver)
    }
}

fn sampler(destination: &str) -> String {
    format!(
        r#"const timer = new QTimer();
timer.interval = {SAMPLE_INTERVAL_MS};
let last = null;
function sample() {{
  const position = workspace.cursorPos;
  if (last && last.x === position.x && last.y === position.y) return;
  last = {{ x: position.x, y: position.y }};
  callDBus("{destination}", "{RECEIVER_PATH}", "{RECEIVER_INTERFACE}", "{RECEIVER_METHOD}", position.x, position.y);
}}
timer.timeout.connect(sample);
timer.start();
sample();
"#
    )
}
