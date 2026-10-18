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
use crab_common::{
    atlas::Animations,
    desktop::{self, Session},
    gpu::{GpuChoice, GpuInfo, GraphicsApi},
};
use snafu::{ResultExt, Snafu, ensure};

use crate::{
    placement::Location,
    renderer::{Renderer, RendererError},
    theme::{Theme, ThemeError},
};

#[cfg(feature = "gnome")]
mod gnome;
#[cfg(feature = "wayland")]
mod wayland;
mod widget;
#[cfg(feature = "x11")]
mod x11;

pub const NAMESPACE: &str = "carb-on-desk";
pub const SIZE: u32 = 200;

#[derive(Debug, Snafu)]
pub enum WindowError {
    #[cfg(feature = "wayland")]
    #[snafu(context(false))]
    Wayland { source: wayland::WaylandError },
    #[cfg(feature = "gnome")]
    #[snafu(context(false))]
    Gnome {
        source: crab_common::gnome::GnomeError,
    },
    #[cfg(feature = "x11")]
    #[snafu(context(false))]
    X11 {
        source: crate::backend::x11::X11Error,
    },
    #[snafu(display("No window backend for this session"))]
    Unsupported,
    #[snafu(display("Renderer error"))]
    #[snafu(context(false))]
    Renderer { source: RendererError },
    #[snafu(display("Theme error"))]
    #[snafu(context(false))]
    Theme { source: ThemeError },
    #[snafu(display("Calloop Error at {thing}"))]
    Calloop {
        source: calloop::Error,
        thing: String,
    },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    Click { side: Side },
    UserIdle { since: Instant },
    UserActive,
    Docked,
    Undocked,
    Hover { inside: bool },
    Moved(Location),
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
    pub fn spawn(
        theme: Theme,
        animation: Animations,
        api: GraphicsApi,
        gpu: GpuChoice,
    ) -> Result<(Self, WindowEvents), WindowError> {
        ensure!(theme.contains(animation), MissingSnafu { animation });
        let theme = Arc::new(theme);
        let window_theme = Arc::clone(&theme);
        let (commands, receiver) = channel::channel();
        let (ready, setup_finished) = mpsc::channel();
        let (event_sender, events) = channel::channel();
        let thread = thread::Builder::new()
            .name("crab-window".to_owned())
            .spawn(move || {
                let window = create(window_theme, event_sender, api, &gpu)?;
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

#[cfg(all(feature = "vulkan", feature = "opengl"))]
fn create(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
    api: GraphicsApi,
    gpu: &GpuChoice,
) -> Result<Box<dyn Window>, WindowError> {
    use crate::renderer::{opengl::Opengl, vulkan::Vulkan};

    match api {
        GraphicsApi::Opengl => open::<Opengl>(theme, events, gpu),
        GraphicsApi::Vulkan => match open::<Vulkan>(Arc::clone(&theme), events.clone(), gpu) {
            Err(WindowError::Renderer {
                source: source @ RendererError::Vulkan { .. },
            }) => {
                eprintln!(
                    "Falling back to OpenGL: {}",
                    snafu::Report::from_error(source)
                );
                open::<Opengl>(theme, events, gpu)
            }
            window => window,
        },
    }
}

#[cfg(not(all(feature = "vulkan", feature = "opengl")))]
fn create(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
    api: GraphicsApi,
    gpu: &GpuChoice,
) -> Result<Box<dyn Window>, WindowError> {
    #[cfg(feature = "opengl")]
    use crate::renderer::opengl::Opengl as Compiled;
    #[cfg(feature = "vulkan")]
    use crate::renderer::vulkan::Vulkan as Compiled;

    let compiled = if cfg!(feature = "vulkan") {
        GraphicsApi::Vulkan
    } else {
        GraphicsApi::Opengl
    };
    if api != compiled {
        eprintln!("This build has no {api:?} renderer, using {compiled:?}");
    }
    open::<Compiled>(theme, events, gpu)
}

fn open<R: Renderer + 'static>(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
    gpu: &GpuChoice,
) -> Result<Box<dyn Window>, WindowError> {
    match desktop::detect() {
        #[cfg(feature = "gnome")]
        Some(Session::Wayland(Some(desktop::Compositor::Gnome))) => {
            gnome::open::<R>(theme, events, gpu)
        }
        #[cfg(feature = "wayland")]
        Some(Session::Wayland(_)) => wayland::open::<R>(theme, events, gpu),
        #[cfg(feature = "x11")]
        Some(Session::X11) => x11::open::<R>(theme, events, gpu),
        _ => UnsupportedSnafu.fail(),
    }
}

#[cfg(feature = "vulkan")]
pub fn gpus() -> Result<Vec<GpuInfo>, WindowError> {
    match desktop::detect() {
        #[cfg(feature = "wayland")]
        Some(Session::Wayland(_)) => wayland::gpus(),
        #[cfg(feature = "x11")]
        Some(Session::X11) => x11::gpus(),
        _ => UnsupportedSnafu.fail(),
    }
}

#[cfg(not(feature = "vulkan"))]
pub const fn gpus() -> Result<Vec<GpuInfo>, WindowError> {
    Ok(Vec::new())
}
