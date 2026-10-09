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
    dirs::{CommonError, get_kwin_cursor_script, get_kwin_placement_script},
    gui::{PERMISSION_APP_ID, PromptSpot},
};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use zbus::{blocking::Connection, interface};

use crate::{
    cursor::{CursorError, CursorSource, Position},
    placement::{self, Location, PlacementError},
};

const KWIN_SERVICE: &str = "org.kde.KWin";
const KWIN_SCRIPTING_PATH: &str = "/Scripting";
const KWIN_SCRIPTING: &str = "org.kde.kwin.Scripting";
const CURSOR_PLUGIN: &str = "crab-on-desk-cursor";
const RECEIVER_PATH: &str = "/com/supernovatux/CrabOnDesk/Cursor";
const RECEIVER_INTERFACE: &str = "com.supernovatux.CrabOnDesk.Cursor";
const RECEIVER_METHOD: &str = "Moved";
const SAMPLE_INTERVAL_MS: u32 = 100;
const PLACEMENT_PLUGIN: &str = "crab-on-desk-placement";
const SPOT_PATH: &str = "/com/supernovatux/CrabOnDesk/Placement";
const SPOT_INTERFACE: &str = "com.supernovatux.CrabOnDesk.Placement";

#[derive(Debug, Snafu)]
pub enum KWinError {
    #[snafu(display("D-Bus error while {thing}"))]
    Bus {
        source: Box<zbus::Error>,
        thing: String,
    },
    #[snafu(display("Unable to write the KWin script {path:?}"))]
    Write { source: io::Error, path: PathBuf },
    #[snafu(display("KWin refused to load the script {plugin}"))]
    Load { plugin: String },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to serve {path}"))]
    Serve {
        source: Box<zbus::Error>,
        path: String,
    },
    #[snafu(display("The session bus gave no name"))]
    Unnamed,
    #[snafu(display("The {thing} lock is poisoned"))]
    Poisoned { thing: String },
}

pub fn connect() -> Result<Connection, KWinError> {
    Connection::session().map_err(Box::new).context(BusSnafu {
        thing: "connecting to the session bus",
    })
}

fn serve<I: zbus::object_server::Interface>(
    connection: &Connection,
    path: &'static str,
    object: I,
) -> Result<String, KWinError> {
    connection
        .object_server()
        .at(path, object)
        .map_err(Box::new)
        .context(ServeSnafu { path })?;
    connection
        .unique_name()
        .map(ToString::to_string)
        .context(UnnamedSnafu)
}

struct Script {
    connection: Connection,
    plugin: &'static str,
    path: PathBuf,
}

impl Script {
    fn load(
        connection: &Connection,
        plugin: &'static str,
        path: PathBuf,
        source: &str,
    ) -> Result<Self, KWinError> {
        fs::write(&path, source).context(WriteSnafu { path: path.clone() })?;
        let script = Self {
            connection: connection.clone(),
            plugin,
            path,
        };
        script.unload()?;
        let id: i32 = script.call(
            KWIN_SCRIPTING_PATH,
            KWIN_SCRIPTING,
            "loadScript",
            &(script.path.to_string_lossy().as_ref(), plugin),
        )?;
        ensure!(id >= 0, LoadSnafu { plugin });
        script.call::<_, ()>(KWIN_SCRIPTING_PATH, KWIN_SCRIPTING, "start", &())?;
        Ok(script)
    }

    fn call<B, R>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<R, KWinError>
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

    fn unload(&self) -> Result<bool, KWinError> {
        self.call(
            KWIN_SCRIPTING_PATH,
            KWIN_SCRIPTING,
            "unloadScript",
            &(self.plugin,),
        )
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        if let Err(error) = self.unload() {
            eprintln!("{}", snafu::Report::from_error(error));
        }
        let _ = fs::remove_file(&self.path);
    }
}

pub struct Cursor {
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

impl Cursor {
    pub fn open() -> Result<Self, KWinError> {
        let latest = Arc::new(Mutex::new(None));
        let connection = connect()?;
        let destination = serve(
            &connection,
            RECEIVER_PATH,
            Receiver {
                latest: Arc::clone(&latest),
            },
        )?;
        let script = Script::load(
            &connection,
            CURSOR_PLUGIN,
            get_kwin_cursor_script()?,
            &sampler(&destination),
        )?;
        Ok(Self {
            latest,
            _script: script,
        })
    }
}

impl CursorSource for Cursor {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        Ok(*self.latest.lock().map_err(|_| KWinError::Poisoned {
            thing: "cursor".to_owned(),
        })?)
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

type KWinSpot = (bool, i32, i32);

pub struct Placer {
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

impl Placer {
    pub fn open() -> Result<Self, KWinError> {
        let spot = Arc::new(Mutex::new(None));
        let connection = connect()?;
        let destination = serve(
            &connection,
            SPOT_PATH,
            SpotServer {
                spot: Arc::clone(&spot),
            },
        )?;
        let script = Script::load(
            &connection,
            PLACEMENT_PLUGIN,
            get_kwin_placement_script()?,
            &placement_script(&destination),
        )?;
        Ok(Self {
            spot,
            _script: script,
        })
    }
}

fn placement_script(destination: &str) -> String {
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

impl placement::Placer for Placer {
    fn place(&mut self, location: &Location) -> Result<Option<PromptSpot>, PlacementError> {
        let PromptSpot { left, edge, middle } = placement::global_spot(location);
        *self.spot.lock().map_err(|_| KWinError::Poisoned {
            thing: "placement spot".to_owned(),
        })? = Some((left, edge, middle));
        Ok(None)
    }
}
