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

use std::{process::Command, thread};

use crab_common::{dirs::get_sibling_executable, gui::SETTINGS_BINARY};
use iced::futures::channel::oneshot;

const ERROR_LINES: usize = 6;
const VANISHED: &str = "The background job stopped unexpectedly";

pub async fn run_settings(args: Vec<String>) -> Result<(), String> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || sender.send(execute(&args)));
    receiver.await.unwrap_or_else(|_| Err(VANISHED.to_owned()))
}

fn execute(args: &[String]) -> Result<(), String> {
    let program = get_sibling_executable(SETTINGS_BINARY)
        .map_err(|error| snafu::Report::from_error(error).to_string())?;
    let output = Command::new(&program)
        .args(args)
        .output()
        .map_err(|error| format!("Unable to run {}: {error}", program.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    let skip = lines.len().saturating_sub(ERROR_LINES);
    Err(lines.into_iter().skip(skip).collect::<Vec<_>>().join("\n"))
}
