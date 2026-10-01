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
    collections::BTreeMap,
    env, fs, io, iter,
    path::{Path, PathBuf},
};

use directories::{BaseDirs, ProjectDirs};
use snafu::{ResultExt, Snafu};

use crate::{
    agent::AGENT_SOCKET,
    atlas::{Animations, MANIFEST_FILE, TEXTURE_EXTENSION},
    claude::{CLAUDE_CONFIG_DIR, CLAUDE_CONFIG_DIR_VAR, CLAUDE_SETTINGS_FILE},
    config::CONFIG_FILE,
    desktop::KWIN_CURSOR_SCRIPT,
    gui::{SETTINGS_LOCK, WIDGET_LOCK},
};

const APP_DIR: &str = "crab-on-desk";
const THEMES_DIR: &str = "themes";
const DATA_DIRS_VAR: &str = "XDG_DATA_DIRS";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

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
    let dir =
        ProjectDirs::from("com", "supernovatux", APP_DIR).ok_or(CommonError::UnableToGetConfig)?;
    Ok(dir.config_local_dir().to_owned())
}
pub fn get_agent_socket() -> Result<PathBuf, CommonError> {
    let dirs = BaseDirs::new().ok_or(CommonError::UnableToGetRuntime)?;
    let runtime = dirs.runtime_dir().ok_or(CommonError::UnableToGetRuntime)?;
    Ok(runtime.join(AGENT_SOCKET))
}

pub fn get_settings_lock() -> Result<PathBuf, CommonError> {
    Ok(get_agent_socket()?.with_file_name(SETTINGS_LOCK))
}

pub fn get_kwin_cursor_script() -> Result<PathBuf, CommonError> {
    Ok(get_agent_socket()?.with_file_name(KWIN_CURSOR_SCRIPT))
}

pub fn get_widget_lock() -> Result<PathBuf, CommonError> {
    Ok(get_agent_socket()?.with_file_name(WIDGET_LOCK))
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

pub fn get_config_file() -> Result<PathBuf, CommonError> {
    Ok(get_config()?.join(CONFIG_FILE))
}

pub fn get_user_themes() -> Result<PathBuf, CommonError> {
    Ok(get_config()?.join(THEMES_DIR))
}

fn get_theme_roots() -> Result<Vec<PathBuf>, CommonError> {
    let data_dirs = env::var_os(DATA_DIRS_VAR)
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    let system = env::split_paths(&data_dirs)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(APP_DIR).join(THEMES_DIR));
    Ok(iter::once(get_user_themes()?).chain(system).collect())
}

pub fn get_theme_dirs() -> Result<BTreeMap<String, PathBuf>, CommonError> {
    let mut themes = BTreeMap::new();
    for root in get_theme_roots()? {
        for (name, dir) in built_themes(&root) {
            themes.entry(name).or_insert(dir);
        }
    }
    Ok(themes)
}

pub fn get_theme_dir(theme: &str) -> Result<Option<PathBuf>, CommonError> {
    Ok(get_theme_dirs()?.remove(theme))
}

fn built_themes(root: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.join(MANIFEST_FILE).is_file())
        .filter_map(|dir| Some((dir.file_name()?.to_str()?.to_owned(), dir)))
        .collect()
}

pub fn get_layer_texture(
    theme_dir: &Path,
    animation: Animations,
    layer: usize,
) -> Result<PathBuf, CommonError> {
    let file = animation
        .layer_file(layer, TEXTURE_EXTENSION)
        .context(AnimationNameSnafu { animation })?;
    Ok(theme_dir.join(file))
}

pub fn get_animation_texture(
    theme_dir: &Path,
    animation: Animations,
) -> Result<PathBuf, CommonError> {
    let file = animation
        .texture_file()
        .context(AnimationNameSnafu { animation })?;
    Ok(theme_dir.join(file))
}
