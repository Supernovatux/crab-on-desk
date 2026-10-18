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

use std::fmt;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

pub const GPUS_COMMAND: &str = "gpus";

const INTEGRATED: &str = "integrated";
const DISCRETE: &str = "discrete";
const PCI_PREFIX: &str = "pci:";
const UUID_PREFIX: &str = "uuid:";

#[derive(Debug, Snafu)]
#[snafu(display(
    "{value:?} is not a GPU choice (integrated, discrete, pci:<address> or uuid:<hex>)"
))]
pub struct GpuChoiceError {
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsApi {
    #[default]
    Vulkan,
    Opengl,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum GpuChoice {
    #[default]
    Integrated,
    Discrete,
    Pci(String),
    Uuid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuKind {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub id: GpuChoice,
    pub name: String,
    pub kind: GpuKind,
    pub driver: String,
    pub compositor: bool,
}

impl TryFrom<String> for GpuChoice {
    type Error = GpuChoiceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == INTEGRATED {
            return Ok(Self::Integrated);
        }
        if value == DISCRETE {
            return Ok(Self::Discrete);
        }
        if let Some(address) = value.strip_prefix(PCI_PREFIX).filter(|id| !id.is_empty()) {
            return Ok(Self::Pci(address.to_owned()));
        }
        if let Some(uuid) = value.strip_prefix(UUID_PREFIX).filter(|id| !id.is_empty()) {
            return Ok(Self::Uuid(uuid.to_owned()));
        }
        GpuChoiceSnafu { value }.fail()
    }
}

impl From<GpuChoice> for String {
    fn from(choice: GpuChoice) -> Self {
        choice.to_string()
    }
}

impl fmt::Display for GpuChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integrated => f.write_str(INTEGRATED),
            Self::Discrete => f.write_str(DISCRETE),
            Self::Pci(address) => write!(f, "{PCI_PREFIX}{address}"),
            Self::Uuid(uuid) => write!(f, "{UUID_PREFIX}{uuid}"),
        }
    }
}

impl fmt::Display for GpuKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Integrated => "integrated",
            Self::Discrete => "discrete",
            Self::Virtual => "virtual",
            Self::Cpu => "software",
            Self::Other => "other",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_through_strings() {
        for choice in [
            GpuChoice::Integrated,
            GpuChoice::Discrete,
            GpuChoice::Pci("0000:01:00.0".to_owned()),
            GpuChoice::Uuid("4daecc90c5b7b7dbdd6ad7cf4a02643f".to_owned()),
        ] {
            assert_eq!(GpuChoice::try_from(choice.to_string()).ok(), Some(choice));
        }
    }

    #[test]
    fn rejects_unknown_choices() {
        assert!(GpuChoice::try_from("fastest".to_owned()).is_err());
        assert!(GpuChoice::try_from("pci:".to_owned()).is_err());
    }
}
