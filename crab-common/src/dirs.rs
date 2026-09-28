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

use std::{env, io, path::PathBuf};

use directories::{BaseDirs, ProjectDirs};
use snafu::{ResultExt, Snafu};

use crate::{
    agent::AGENT_SOCKET,
    atlas::{Animations, BEHAVIOUR_FILE, MANIFEST_FILE},
    claude::{CLAUDE_CONFIG_DIR, CLAUDE_CONFIG_DIR_VAR, CLAUDE_SETTINGS_FILE},
    config::CONFIG_FILE,
};

const THEMES_DIR: &str = "themes";

#[derive(Debug, Snafu)]
pub enum CommonError {
    UnableToGetConfig,
    UnableToGetCache,
    UnableToGetRuntime,
    UnableToGetHome,
    #[snafu(display("Unable to locate the running executable"))]
    CurrentExe {
        source: io::Error,
    },
    #[snafu(display("Unable to name {animation:?}"))]
    AnimationName {
        source: serde_plain::Error,
        animation: Animations,
    },
}

pub fn get_config() -> Result<PathBuf, CommonError> {
    let dir = ProjectDirs::from("com", "supernovatux", "crab-on-desk")
        .ok_or(CommonError::UnableToGetConfig)?;
    Ok(dir.config_local_dir().to_owned())
}
pub fn get_cache() -> Result<PathBuf, CommonError> {
    let dir = ProjectDirs::from("com", "supernovatux", "crab-on-desk")
        .ok_or(CommonError::UnableToGetCache)?;
    Ok(dir.cache_dir().to_owned())
}

pub fn get_agent_socket() -> Result<PathBuf, CommonError> {
    let dirs = BaseDirs::new().ok_or(CommonError::UnableToGetRuntime)?;
    let runtime = dirs.runtime_dir().ok_or(CommonError::UnableToGetRuntime)?;
    Ok(runtime.join(AGENT_SOCKET))
}

pub fn get_claude_settings() -> Result<PathBuf, CommonError> {
    let dir = match env::var_os(CLAUDE_CONFIG_DIR_VAR) {
        Some(dir) => PathBuf::from(dir),
        None => BaseDirs::new()
            .ok_or(CommonError::UnableToGetHome)?
            .home_dir()
            .join(CLAUDE_CONFIG_DIR),
    };
    Ok(dir.join(CLAUDE_SETTINGS_FILE))
}

pub fn get_sibling_executable(name: &str) -> Result<PathBuf, CommonError> {
    Ok(env::current_exe()
        .context(CurrentExeSnafu)?
        .with_file_name(name))
}

pub fn get_theme_sources() -> Result<PathBuf, CommonError> {
    Ok(get_config()?.join(THEMES_DIR))
}

pub fn get_theme_source(theme: &str) -> Result<PathBuf, CommonError> {
    Ok(get_theme_sources()?.join(theme))
}

pub fn get_animation_source(theme: &str, animation: Animations) -> Result<PathBuf, CommonError> {
    let file = animation
        .source_file()
        .context(AnimationNameSnafu { animation })?;
    Ok(get_theme_source(theme)?.join(file))
}

pub fn get_theme_behaviour(theme: &str) -> Result<PathBuf, CommonError> {
    let mut path = get_theme_sources()?;
    path.push(theme);
    path.push(BEHAVIOUR_FILE);
    Ok(path)
}

pub fn get_theme_cache(theme: &str) -> Result<PathBuf, CommonError> {
    let mut dir = get_cache()?;
    dir.push(THEMES_DIR);
    dir.push(theme);
    Ok(dir)
}

pub fn get_config_file() -> Result<PathBuf, CommonError> {
    Ok(get_config()?.join(CONFIG_FILE))
}

pub fn get_theme_manifest(theme: &str) -> Result<PathBuf, CommonError> {
    Ok(get_theme_cache(theme)?.join(MANIFEST_FILE))
}

pub fn get_animation_texture(theme: &str, animation: Animations) -> Result<PathBuf, CommonError> {
    let file = animation
        .texture_file()
        .context(AnimationNameSnafu { animation })?;
    Ok(get_theme_cache(theme)?.join(file))
}
