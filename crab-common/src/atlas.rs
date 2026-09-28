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

use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "meta.toml";
pub const TEXTURE_EXTENSION: &str = "ktx2";
pub const SOURCE_EXTENSION: &str = "apng";
pub const BEHAVIOUR_FILE: &str = "theme.toml";

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Animations {
    #[default]
    Idle,
    Roam,
    Yawning,
    Dozing,
    Collapsing,
    Thinking,
    Working,
    Juggling,
    #[serde(rename = "working-tier-1")]
    WorkingTier1,
    #[serde(rename = "working-tier-2")]
    WorkingTier2,
    #[serde(rename = "working-tier-3")]
    WorkingTier3,
    #[serde(rename = "juggling-tier-1")]
    JugglingTier1,
    #[serde(rename = "juggling-tier-2")]
    JugglingTier2,
    #[serde(rename = "juggling-tier-3")]
    JugglingTier3,
    #[serde(rename = "idle-pool-1")]
    IdlePool1,
    #[serde(rename = "idle-pool-2")]
    IdlePool2,
    #[serde(rename = "idle-pool-3")]
    IdlePool3,
    #[serde(rename = "idle-pool-4")]
    IdlePool4,
    Sweeping,
    Error,
    Attention,
    Notification,
    Carrying,
    Sleeping,
    Waking,
    Dizzy,
    MiniIdle,
    MiniAlert,
    MiniHappy,
    MiniEnter,
    MiniPeek,
    MiniWorking,
    MiniCrabwalk,
    MiniEnterSleep,
    MiniSleep,
    ReactDrag,
    ReactLeft,
    ReactRight,
    ReactAnnoyed,
    #[serde(rename = "react-double-1")]
    ReactDouble1,
    #[serde(rename = "react-double-2")]
    ReactDouble2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManifest {
    pub behaviour: ThemeBehaviour,
    pub animations: BTreeMap<Animations, AnimationInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeBehaviour {
    pub idle_pool: Vec<IdleAnimation>,
    pub react_double: Vec<Animations>,
    pub working_tiers: Vec<Tier>,
    pub juggling_tiers: Vec<Tier>,
    pub min_display_ms: BTreeMap<Animations, u32>,
    pub auto_return_ms: BTreeMap<Animations, u32>,
    pub reaction_ms: BTreeMap<Animations, u32>,
    pub sleep: SleepTimings,
    pub mini: Option<Mini>,
    pub roam_flip_assets: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Mini {
    pub offset_ratio: f64,
    #[serde(default)]
    pub flip_assets: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IdleAnimation {
    pub animation: Animations,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SleepTimings {
    pub idle_after_ms: u32,
    pub yawn_after_ms: u32,
    pub deep_sleep_after_ms: u32,
    pub yawn_ms: u32,
    pub collapse_ms: Option<u32>,
    pub wake_ms: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tier {
    pub min_sessions: u32,
    pub animation: Animations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationInfo {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub frame_delays_ms: Vec<u32>,
    pub loops: bool,
    pub hitbox: Rect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ThemeBehaviour {
    pub fn animations(&self) -> impl Iterator<Item = Animations> {
        self.idle_pool
            .iter()
            .map(|idle| idle.animation)
            .chain(self.react_double.iter().copied())
            .chain(self.reaction_ms.keys().copied())
            .chain(
                self.working_tiers
                    .iter()
                    .chain(&self.juggling_tiers)
                    .map(|tier| tier.animation),
            )
    }
}

impl Animations {
    #[must_use]
    pub const fn is_mini(self) -> bool {
        matches!(
            self,
            Self::MiniIdle
                | Self::MiniAlert
                | Self::MiniHappy
                | Self::MiniEnter
                | Self::MiniPeek
                | Self::MiniWorking
                | Self::MiniCrabwalk
                | Self::MiniEnterSleep
                | Self::MiniSleep
        )
    }

    pub fn source_file(&self) -> Result<String, serde_plain::Error> {
        serde_plain::to_string(self).map(|name| format!("{name}.{SOURCE_EXTENSION}"))
    }

    pub fn texture_file(&self) -> Result<String, serde_plain::Error> {
        serde_plain::to_string(self).map(|name| format!("{name}.{TEXTURE_EXTENSION}"))
    }
}

impl FromStr for Animations {
    type Err = serde_plain::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_plain::from_str(s)
    }
}
