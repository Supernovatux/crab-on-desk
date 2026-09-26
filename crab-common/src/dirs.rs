use std::path::PathBuf;

use directories::ProjectDirs;
use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum CommonError {
    UnableToGetConfig,
    UnableToGetCache,
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

pub fn get_theme_cache(theme: &str) -> Result<PathBuf, CommonError> {
    let mut dir = get_cache()?;
    dir.push("themes");
    dir.push(theme);
    Ok(dir)
}
