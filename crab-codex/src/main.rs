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

use std::env;

use crab_common::{
    config::Config,
    control::{Control, Message},
    ipc::{self, IpcError},
};
use snafu::Snafu;

mod pet;
mod source;
mod theme;

#[derive(Debug, Snafu)]
enum CodexError {
    #[snafu(display("Unable to get the pet package"))]
    #[snafu(context(false))]
    Source { source: source::SourceError },
    #[snafu(display("Invalid pet package"))]
    #[snafu(context(false))]
    Pet { source: pet::PetError },
    #[snafu(display("Unable to install the theme"))]
    #[snafu(context(false))]
    Theme { source: theme::ThemeError },
    #[snafu(display("Unable to ask the widget to reload"))]
    #[snafu(context(false))]
    Ipc { source: IpcError },
    #[snafu(display(
        "Usage: crab-codex <codex-pets.net link | https .zip URL | pet id | local .zip or folder>"
    ))]
    Usage,
}

#[snafu::report]
fn main() -> Result<(), CodexError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let [input] = args.as_slice() else {
        return UsageSnafu.fail();
    };
    let pet = pet::Pet::load(source::fetch(input)?)?;
    let dir = theme::install(&pet)?;
    println!(
        "Installed {} as theme {} in {}",
        pet.display_name,
        pet.theme,
        dir.display()
    );
    if Config::load().is_ok_and(|config| config.default_theme == pet.theme)
        && let Some(mut stream) = ipc::connect()?
    {
        ipc::send(&mut stream, &Message::Control(Control::Reload))?;
    }
    Ok(())
}
