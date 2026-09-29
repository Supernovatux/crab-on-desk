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
    env, io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::Instant,
};

use crab_common::{
    atlas::Animations,
    config::{Config, ConfigError},
    dirs::{CommonError, get_sibling_executable},
    gui::{GUI_BINARY, INIT_MODE},
};
use crab_widget::{
    handler::{self, HandlerError, Outcome},
    state::StateMachine,
    theme::{Theme, ThemeError},
    window::{WindowError, WindowHandle},
};
use snafu::{ResultExt, Snafu};

const REPLACED_SUFFIX: &str = " (deleted)";

#[derive(Debug, Snafu)]
enum WidgetError {
    #[snafu(display("config error"))]
    #[snafu(context(false))]
    Config { source: ConfigError },
    #[snafu(display("window error"))]
    #[snafu(context(false))]
    Window { source: WindowError },
    #[snafu(display("theme error"))]
    #[snafu(context(false))]
    Theme { source: ThemeError },
    #[snafu(display("handler error"))]
    #[snafu(context(false))]
    Handler { source: HandlerError },
    #[snafu(display("Unable to restart the widget"))]
    Restart { source: io::Error },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to start {GUI_BINARY} {INIT_MODE}"))]
    Init { source: io::Error },
}

#[snafu::report]
fn main() -> Result<(), WidgetError> {
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{}", snafu::Report::from_error(error));
            return launch_init();
        }
    };
    let theme = Theme::load(config.theme_dir()?)?;
    let state = StateMachine::new(
        theme.behaviour().clone(),
        theme.clip_lengths(),
        config.free_roam,
        Instant::now(),
    );
    let (window, window_events) = WindowHandle::spawn(theme, Animations::default())?;
    if handler::run(window, window_events, state)? == Outcome::Restart {
        return Err(restart()).context(RestartSnafu);
    }
    Ok(())
}

fn launch_init() -> Result<(), WidgetError> {
    Command::new(get_sibling_executable(GUI_BINARY)?)
        .arg(INIT_MODE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context(InitSnafu)?;
    Ok(())
}

fn restart() -> io::Error {
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return error,
    };
    let executable = executable
        .to_str()
        .and_then(|path| path.strip_suffix(REPLACED_SUFFIX))
        .map_or_else(|| executable.clone(), PathBuf::from);
    Command::new(executable).args(env::args_os().skip(1)).exec()
}
