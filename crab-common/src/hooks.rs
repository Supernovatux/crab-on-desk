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
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use crate::{
    agent::{Agent, HOOK_BINARY},
    claude::HookEvent,
    dirs::{CommonError, get_claude_settings, get_sibling_executable},
};
use serde_json::{Map, Value, json};
use snafu::{OptionExt, ResultExt, Snafu, ensure};

const HOOKS_KEY: &str = "hooks";
const COMMAND_KEY: &str = "command";
const PERMISSION_TIMEOUT_SECONDS: u32 = 600;
const BACKUP_SUFFIX: &str = ".crab-on-desk.bak";

#[derive(Debug, Snafu)]
pub enum HooksError {
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("{path:?} not found, build it with `cargo build --release -p crab-hook`"))]
    MissingHook { path: PathBuf },
    #[snafu(display("{path:?} is not valid UTF-8"))]
    NonUtf8 { path: PathBuf },
    #[snafu(display("Unable to name {thing}"))]
    Name {
        source: serde_plain::Error,
        thing: String,
    },
    #[snafu(display("Unable to read {path:?}"))]
    Read { source: io::Error, path: PathBuf },
    #[snafu(display("Invalid JSON in {path:?}"))]
    Parse {
        source: serde_json::Error,
        path: PathBuf,
    },
    #[snafu(display("Unexpected shape of {thing} in {path:?}"))]
    Shape { thing: String, path: PathBuf },
    #[snafu(display("Unable to back up {path:?}"))]
    Backup { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to write {path:?}"))]
    Write { source: io::Error, path: PathBuf },
    #[snafu(display("Unable to encode {path:?}"))]
    Encode {
        source: serde_json::Error,
        path: PathBuf,
    },
}

pub fn install() -> Result<(), HooksError> {
    let hook = get_sibling_executable(HOOK_BINARY)?;
    ensure!(hook.is_file(), MissingHookSnafu { path: hook });
    let agent = serde_plain::to_string(&Agent::Claude).context(NameSnafu {
        thing: format!("{:?}", Agent::Claude),
    })?;
    let command = format!("{} {agent}", shell_quote(&hook)?);
    edit_settings(|hooks, path| {
        remove_own(hooks);
        add_own(hooks, &command, path)
    })
}

pub fn uninstall() -> Result<(), HooksError> {
    edit_settings(|hooks, _| {
        remove_own(hooks);
        Ok(())
    })
}

pub fn installed() -> Result<bool, HooksError> {
    let path = get_claude_settings()?;
    let Some(settings) = read_settings(&path)? else {
        return Ok(false);
    };
    Ok(settings
        .get(HOOKS_KEY)
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::values)
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get(HOOKS_KEY).and_then(Value::as_array))
        .flatten()
        .any(is_own))
}

fn read_settings(path: &Path) -> Result<Option<Value>, HooksError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context(ReadSnafu { path }),
    };
    serde_json::from_str(&text)
        .map(Some)
        .context(ParseSnafu { path })
}

fn edit_settings(
    edit: impl FnOnce(&mut Map<String, Value>, &Path) -> Result<(), HooksError>,
) -> Result<(), HooksError> {
    let path = get_claude_settings()?;
    let original = read_settings(&path)?;
    let unchanged = original.clone().unwrap_or_else(|| json!({}));
    let mut settings = unchanged.clone();
    let hooks = settings
        .as_object_mut()
        .context(ShapeSnafu {
            thing: "the settings",
            path: path.clone(),
        })?
        .entry(HOOKS_KEY)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context(ShapeSnafu {
            thing: HOOKS_KEY,
            path: path.clone(),
        })?;
    edit(hooks, &path)?;
    if settings == unchanged {
        return Ok(());
    }
    if original.is_some() {
        let mut backup = path.clone().into_os_string();
        backup.push(BACKUP_SUFFIX);
        fs::copy(&path, &backup).context(BackupSnafu { path: backup })?;
    }
    let mut text =
        serde_json::to_string_pretty(&settings).context(EncodeSnafu { path: path.clone() })?;
    text.push('\n');
    fs::write(&path, text).context(WriteSnafu { path })
}

fn remove_own(hooks: &mut Map<String, Value>) {
    hooks.retain(|_, groups| {
        let Some(groups) = groups.as_array_mut() else {
            return true;
        };
        let before = groups.len();
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut(HOOKS_KEY).and_then(Value::as_array_mut) else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|handler| !is_own(handler));
            handlers.len() == before || !handlers.is_empty()
        });
        groups.len() == before || !groups.is_empty()
    });
}

fn add_own(hooks: &mut Map<String, Value>, command: &str, path: &Path) -> Result<(), HooksError> {
    for event in HookEvent::ALL {
        let name = serde_plain::to_string(&event).context(NameSnafu {
            thing: format!("{event:?}"),
        })?;
        let handler = if event.is_blocking() {
            json!({ "type": "command", COMMAND_KEY: command, "timeout": PERMISSION_TIMEOUT_SECONDS })
        } else {
            json!({ "type": "command", COMMAND_KEY: command, "async": true })
        };
        hooks
            .entry(name.clone())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context(ShapeSnafu {
                thing: format!("{HOOKS_KEY}.{name}"),
                path,
            })?
            .push(json!({ HOOKS_KEY: [handler] }));
    }
    Ok(())
}

fn is_own(handler: &Value) -> bool {
    handler
        .get(COMMAND_KEY)
        .and_then(Value::as_str)
        .and_then(executable)
        .and_then(|program| Path::new(program).file_name())
        .is_some_and(|name| name == HOOK_BINARY)
}

fn executable(command: &str) -> Option<&str> {
    command.strip_prefix('\'').map_or_else(
        || command.split_whitespace().next(),
        |quoted| quoted.split_once('\'').map(|(program, _)| program),
    )
}

fn shell_quote(path: &Path) -> Result<String, HooksError> {
    let text = path.to_str().context(NonUtf8Snafu { path })?;
    Ok(format!("'{}'", text.replace('\'', r"'\''")))
}
