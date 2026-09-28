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

use serde::{Serialize, de::DeserializeOwned};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum TomlFileError {
    #[snafu(display("Unable to read {path:?}"))]
    Read { source: io::Error, path: PathBuf },
    #[snafu(display("Invalid toml in {path:?}"))]
    Parse {
        source: toml::de::Error,
        path: PathBuf,
    },
    #[snafu(display("Unable to encode {path:?}"))]
    Encode {
        source: toml::ser::Error,
        path: PathBuf,
    },
    #[snafu(display("Unable to write {path:?}"))]
    Write { source: io::Error, path: PathBuf },
}

pub fn read<T: DeserializeOwned>(path: PathBuf) -> Result<T, TomlFileError> {
    let text = fs::read_to_string(&path).context(ReadSnafu { path: path.clone() })?;
    toml::from_str(&text).context(ParseSnafu { path })
}

pub fn write<T: Serialize>(path: PathBuf, value: &T) -> Result<(), TomlFileError> {
    let text = toml::to_string_pretty(value).context(EncodeSnafu { path: path.clone() })?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).context(WriteSnafu { path: dir })?;
    }
    fs::write(&path, text).context(WriteSnafu { path })
}
