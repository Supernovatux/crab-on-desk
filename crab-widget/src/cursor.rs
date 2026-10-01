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
    fs, io,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crab_common::{
    desktop::{self, Compositor},
    dirs::{CommonError, get_kwin_cursor_script},
};
use hyprland::{data::CursorPosition, error::HyprError, shared::HyprData};
use snafu::{ResultExt, Snafu, ensure};
use zbus::{blocking::Connection, interface};

const KWIN_SERVICE: &str = "org.kde.KWin";
const KWIN_SCRIPTING_PATH: &str = "/Scripting";
const KWIN_SCRIPTING: &str = "org.kde.kwin.Scripting";
const KWIN_SCRIPT: &str = "org.kde.kwin.Script";
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
    #[snafu(display("D-Bus error while {thing}"))]
    Bus {
        source: Box<zbus::Error>,
        thing: String,
    },
    #[snafu(display("Unable to write the KWin script {path:?}"))]
    Script { source: io::Error, path: PathBuf },
    #[snafu(display("KWin refused to load the cursor script"))]
    Load,
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
    connection: Connection,
    latest: Arc<Mutex<Option<Position>>>,
    script: PathBuf,
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
        let connection = Connection::session().map_err(Box::new).context(BusSnafu {
            thing: "connecting to the session bus",
        })?;
        connection
            .object_server()
            .at(
                RECEIVER_PATH,
                Receiver {
                    latest: Arc::clone(&latest),
                },
            )
            .map_err(Box::new)
            .context(BusSnafu {
                thing: "serving the cursor receiver",
            })?;
        let destination = connection
            .unique_name()
            .map(ToString::to_string)
            .ok_or(CursorError::Receiver)?;
        let script = get_kwin_cursor_script()?;
        fs::write(&script, sampler(&destination)).context(ScriptSnafu {
            path: script.clone(),
        })?;
        let kwin = Self {
            connection,
            latest,
            script,
        };
        kwin.unload()?;
        let id: i32 = kwin.call(
            KWIN_SCRIPTING_PATH,
            KWIN_SCRIPTING,
            "loadScript",
            &(kwin.script.to_string_lossy().as_ref(), KWIN_PLUGIN),
        )?;
        ensure!(id >= 0, LoadSnafu);
        kwin.call::<_, ()>(&format!("/Scripting/Script{id}"), KWIN_SCRIPT, "run", &())?;
        Ok(kwin)
    }

    fn call<B, R>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<R, CursorError>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        self.connection
            .call_method(Some(KWIN_SERVICE), path, Some(interface), method, body)
            .and_then(|reply| reply.body().deserialize())
            .map_err(Box::new)
            .context(BusSnafu {
                thing: format!("calling {interface}.{method}"),
            })
    }

    fn unload(&self) -> Result<bool, CursorError> {
        self.call(
            KWIN_SCRIPTING_PATH,
            KWIN_SCRIPTING,
            "unloadScript",
            &(KWIN_PLUGIN,),
        )
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

impl Drop for KWin {
    fn drop(&mut self) {
        if let Err(error) = self.unload() {
            eprintln!("{}", snafu::Report::from_error(error));
        }
        let _ = fs::remove_file(&self.script);
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
