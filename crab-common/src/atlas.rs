use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "meta.toml";
pub const TEXTURE_EXTENSION: &str = "ktx2";

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Animations {
    Attention,
    Bubble,
    Building,
    Carrying,
    Conducting,
    Debugger,
    Error,
    Happy,
    HeadphonesGroove,
    Idle,
    IdleReading,
    Juggling,
    MiniAlert,
    MiniCrabwalk,
    MiniEnter,
    MiniHappy,
    MiniIdle,
    MiniWorking,
    MiniPeek,
    MiniSleep,
    Notification,
    ReactAnnoyed,
    ReactDoubleJump,
    Sleeping,
    Sweeping,
    Thinking,
    Typing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManifest {
    pub animations: BTreeMap<Animations, AnimationInfo>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AnimationInfo {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
}

impl Animations {
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
