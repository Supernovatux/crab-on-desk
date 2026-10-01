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
    io,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Instant,
};

use calloop::channel::{self, Channel, Sender};
use crab_common::atlas::Animations;
use snafu::{ResultExt, Snafu, ensure};

use crate::theme::Theme;

mod wayland;

#[derive(Debug, Snafu)]
pub enum WindowError {
    #[snafu(context(false))]
    Wayland { source: wayland::WaylandError },
    #[snafu(display("Unable to spawn window thread"))]
    Spawn { source: io::Error },
    #[snafu(display("Window thread exited before setup finished"))]
    Setup,
    #[snafu(display("Window thread panicked"))]
    Panicked,
    #[snafu(display("Window is closed"))]
    Closed,
    #[snafu(display("Theme has no {animation:?} animation"))]
    Missing { animation: Animations },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowEvent {
    Click { side: Side },
    UserIdle { since: Instant },
    UserActive,
    Docked,
    Undocked,
    Hover { inside: bool },
    Moved { center: (f64, f64) },
    RoamEnded,
    OpenSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    Show(Animations),
    MoveToOutput(String),
    Roam(bool),
    Look(Option<(f64, f64)>),
}

trait Window {
    fn new(theme: Arc<Theme>, events: Sender<WindowEvent>) -> Result<Box<Self>, WindowError>
    where
        Self: Sized;
    fn run(
        self: Box<Self>,
        animation: Animations,
        commands: Channel<WindowCommand>,
    ) -> Result<(), WindowError>;
}

pub type WindowEvents = Channel<WindowEvent>;

pub struct WindowHandle {
    theme: Arc<Theme>,
    commands: Sender<WindowCommand>,
    thread: JoinHandle<Result<(), WindowError>>,
}

impl WindowHandle {
    pub fn spawn(theme: Theme, animation: Animations) -> Result<(Self, WindowEvents), WindowError> {
        ensure!(theme.contains(animation), MissingSnafu { animation });
        let theme = Arc::new(theme);
        let window_theme = Arc::clone(&theme);
        let (commands, receiver) = channel::channel();
        let (ready, setup_finished) = mpsc::channel();
        let (event_sender, events) = channel::channel();
        let thread = thread::Builder::new()
            .name("crab-window".to_owned())
            .spawn(move || {
                let window = create(window_theme, event_sender)?;
                ready.send(()).map_err(|_| WindowError::Closed)?;
                window.run(animation, receiver)
            })
            .context(SpawnSnafu)?;
        if setup_finished.recv().is_ok() {
            return Ok((
                Self {
                    theme,
                    commands,
                    thread,
                },
                events,
            ));
        }
        join(thread)?;
        Err(WindowError::Setup)
    }

    pub fn set_animation(&self, animation: Animations) -> Result<(), WindowError> {
        ensure!(self.theme.contains(animation), MissingSnafu { animation });
        self.send(WindowCommand::Show(animation))
    }

    pub fn move_to_output(&self, output: String) -> Result<(), WindowError> {
        self.send(WindowCommand::MoveToOutput(output))
    }

    pub fn set_roaming(&self, roaming: bool) -> Result<(), WindowError> {
        self.send(WindowCommand::Roam(roaming))
    }

    pub fn look_at(&self, cursor: Option<(f64, f64)>) -> Result<(), WindowError> {
        self.send(WindowCommand::Look(cursor))
    }

    fn send(&self, command: WindowCommand) -> Result<(), WindowError> {
        self.commands.send(command).map_err(|_| WindowError::Closed)
    }

    pub fn join(self) -> Result<(), WindowError> {
        let Self {
            commands, thread, ..
        } = self;
        drop(commands);
        join(thread)
    }
}

fn join(thread: JoinHandle<Result<(), WindowError>>) -> Result<(), WindowError> {
    thread.join().map_err(|_| WindowError::Panicked)?
}

fn create(theme: Arc<Theme>, events: Sender<WindowEvent>) -> Result<Box<dyn Window>, WindowError> {
    #[cfg(target_os = "linux")]
    {
        use crate::{renderer::opengl::Opengl, window::wayland::WaylandWindow};

        let window = WaylandWindow::<Opengl>::new(theme, events)?;
        Ok(window)
    }
}
