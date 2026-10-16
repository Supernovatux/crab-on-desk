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
    thread,
    time::{Duration, Instant},
};

use calloop::channel::Sender;
use crab_common::{
    gnome::{self, GnomeError, Monitor, ShellProxyBlocking},
    gui::PromptSpot,
};
use zbus::proxy;

use crate::{
    cursor::{CursorError, CursorSource, Position},
    placement::{self, Location, PlacementError},
};

#[proxy(
    interface = "org.gnome.Mutter.IdleMonitor",
    default_service = "org.gnome.Mutter.IdleMonitor",
    default_path = "/org/gnome/Mutter/IdleMonitor/Core"
)]
trait IdleMonitor {
    fn add_idle_watch(&self, interval_ms: u64) -> zbus::Result<u32>;
    fn add_user_active_watch(&self) -> zbus::Result<u32>;
    #[zbus(signal)]
    fn watch_fired(&self, id: u32) -> zbus::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Moved(Monitor),
    UserIdle { since: Instant },
    UserActive,
}

pub fn forward_moves(sender: Sender<Signal>) -> Result<(), GnomeError> {
    let shell = gnome::shell()?;
    let moves = gnome::reply(shell.receive_moved(), "subscribing to Moved")?;
    spawn("crab-gnome-moves", move || {
        for signal in moves {
            let Ok(args) = signal.args() else { continue };
            let monitor = (args.connector, args.monitor);
            if sender.send(Signal::Moved(monitor)).is_err() {
                break;
            }
        }
    })
}

pub fn forward_idle(timeout: Duration, sender: Sender<Signal>) -> Result<(), GnomeError> {
    let proxy = IdleMonitorProxyBlocking::new(&gnome::connection()?)
        .map_err(Box::new)
        .map_err(|source| GnomeError::Bus {
            source,
            thing: "creating the idle monitor proxy".to_owned(),
        })?;
    let fired = gnome::reply(proxy.receive_watch_fired(), "subscribing to WatchFired")?;
    let idle = gnome::reply(
        proxy.add_idle_watch(timeout.as_millis() as u64),
        "adding the idle watch",
    )?;
    let mut active = None;
    spawn("crab-gnome-idle", move || {
        for signal in fired {
            let Ok(args) = signal.args() else { continue };
            let event = if args.id == idle {
                active = proxy.add_user_active_watch().ok();
                let now = Instant::now();
                Signal::UserIdle {
                    since: now.checked_sub(timeout).unwrap_or(now),
                }
            } else if Some(args.id) == active {
                active = None;
                Signal::UserActive
            } else {
                continue;
            };
            if sender.send(event).is_err() {
                break;
            }
        }
    })
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), GnomeError> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(drop)
        .map_err(|source| GnomeError::Spawn {
            source,
            thing: name.to_owned(),
        })
}

pub struct Cursor {
    shell: ShellProxyBlocking<'static>,
}

impl Cursor {
    pub fn open() -> Result<Self, GnomeError> {
        Ok(Self {
            shell: gnome::shell()?,
        })
    }
}

impl CursorSource for Cursor {
    fn position(&mut self) -> Result<Option<Position>, CursorError> {
        Ok(Some(gnome::reply(
            self.shell.cursor_position(),
            "reading the cursor",
        )?))
    }
}

pub struct Placer {
    shell: ShellProxyBlocking<'static>,
}

impl Placer {
    pub fn open() -> Result<Self, GnomeError> {
        Ok(Self {
            shell: gnome::shell()?,
        })
    }
}

impl placement::Placer for Placer {
    fn place(&mut self, location: &Location) -> Result<Option<PromptSpot>, PlacementError> {
        let PromptSpot { left, edge, middle } = placement::global_spot(location);
        gnome::reply(
            self.shell.set_prompt_spot(left, edge, middle),
            "placing the permission prompt",
        )?;
        Ok(None)
    }
}
