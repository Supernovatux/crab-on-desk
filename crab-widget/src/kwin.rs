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

use std::{fs, io, path::PathBuf};

use snafu::{ResultExt, Snafu, ensure};
use zbus::blocking::Connection;

const KWIN_SERVICE: &str = "org.kde.KWin";
const KWIN_SCRIPTING_PATH: &str = "/Scripting";
const KWIN_SCRIPTING: &str = "org.kde.kwin.Scripting";

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
}

pub fn connect() -> Result<Connection, KWinError> {
    Connection::session().map_err(Box::new).context(BusSnafu {
        thing: "connecting to the session bus",
    })
}

pub struct Script {
    connection: Connection,
    plugin: &'static str,
    path: PathBuf,
}

impl Script {
    pub fn load(
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
