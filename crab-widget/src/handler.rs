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
    collections::{HashMap, HashSet},
    os::unix::{net::UnixStream, process::CommandExt},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, Interest, LoopHandle, Mode, PostAction, RegistrationToken, channel,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};
use crab_common::{
    agent::{Agent, AgentEvent, EventKind},
    atlas::Animations,
    claude::CLAUDE_PROCESS,
    control::{Control, Message},
    dirs::get_sibling_executable,
    gui::{DISPLAYS_PAGE, GUI_BINARY, SETTINGS_MODE},
};
use hyprland::{data::CursorPosition, shared::HyprData};
use rustix::{
    io::Errno,
    process::{Pid, PidfdFlags, pidfd_open},
};
use snafu::{ResultExt, Snafu};

use crate::{
    agent::{self, AgentError},
    clicks::Clicks,
    permission::{self, Prompt},
    process,
    random::roll,
    spin::Spin,
    state::StateMachine,
    window::{Side, WindowError, WindowEvent, WindowEvents, WindowHandle},
};

const CURSOR_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Snafu)]
pub enum HandlerError {
    #[snafu(context(false))]
    Agent { source: AgentError },
    #[snafu(context(false))]
    Window { source: WindowError },
    #[snafu(display("Calloop error at {thing}"))]
    Calloop {
        source: calloop::Error,
        thing: String,
    },
}

enum Watch {
    Watching(RegistrationToken),
    AlreadyExited,
    Unwatchable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Closed,
    Restart,
}

struct Handler {
    window: WindowHandle,
    state: StateMachine,
    clicks: Clicks,
    handle: LoopHandle<'static, Self>,
    timer: Option<(Instant, RegistrationToken)>,
    watched: HashSet<u32>,
    prompts: HashMap<u64, Prompt>,
    next_prompt: u64,
    settings: HashMap<u32, Child>,
    roaming: bool,
    cursor: Cursor,
    running: bool,
    outcome: Outcome,
}

#[derive(Debug, Default)]
struct Cursor {
    polling: bool,
    unavailable: bool,
    last: Option<(i64, i64)>,
    center: Option<(f64, f64)>,
    spin: Spin,
}

pub fn run(
    window: WindowHandle,
    window_events: WindowEvents,
    state: StateMachine,
) -> Result<Outcome, HandlerError> {
    let mut event_loop: EventLoop<Handler> = EventLoop::try_new().context(CalloopSnafu {
        thing: "creating event loop",
    })?;
    let _socket = agent::listen(
        &event_loop.handle(),
        |message, stream, handler: &mut Handler| match message {
            Message::Agent(event) => handler.on_agent_event(&event, stream),
            Message::Control(Control::Reload) => {
                handler.outcome = Outcome::Restart;
                handler.running = false;
            }
            Message::Control(Control::MoveToOutput { output }) => {
                if handler.window.move_to_output(output).is_err() {
                    handler.running = false;
                }
            }
        },
    )?;
    event_loop
        .handle()
        .insert_source(window_events, |event, (), handler| match event {
            channel::Event::Msg(WindowEvent::Click { side }) => handler.on_click(side),
            channel::Event::Msg(WindowEvent::UserIdle { since }) => {
                handler.state.on_user_idle(since);
                handler.show(None);
            }
            channel::Event::Msg(WindowEvent::UserActive) => {
                let animation = handler.state.on_user_active(Instant::now());
                handler.show(animation);
            }
            channel::Event::Msg(WindowEvent::Docked) => {
                let animation = handler.state.dock(Instant::now());
                handler.show(animation);
            }
            channel::Event::Msg(WindowEvent::Undocked) => {
                let animation = handler.state.undock(Instant::now());
                handler.show(animation);
            }
            channel::Event::Msg(WindowEvent::Hover { inside }) => {
                let animation = handler.state.hover(inside, Instant::now());
                handler.show(animation);
            }
            channel::Event::Msg(WindowEvent::Moved { center }) => {
                handler.cursor.center = Some(center);
            }
            channel::Event::Msg(WindowEvent::OpenSettings) => handler.open_settings(),
            channel::Event::Msg(WindowEvent::RoamEnded) => {
                handler.roaming = false;
                let animation = handler.state.end_roam(Instant::now());
                handler.show(animation);
            }
            channel::Event::Closed => handler.running = false,
        })
        .map_err(|e| e.error)
        .context(CalloopSnafu {
            thing: "adding window event channel",
        })?;
    let signal = event_loop.get_signal();
    let mut handler = Handler {
        window,
        state,
        clicks: Clicks::default(),
        handle: event_loop.handle(),
        timer: None,
        watched: HashSet::new(),
        prompts: HashMap::new(),
        next_prompt: 0,
        settings: HashMap::new(),
        roaming: false,
        cursor: Cursor::default(),
        running: true,
        outcome: Outcome::Closed,
    };
    let running_agents = process::find(CLAUDE_PROCESS);
    handler
        .state
        .recover(running_agents.iter().copied(), Instant::now());
    for pid in running_agents {
        handler.watch(pid);
    }
    handler.show(None);
    let result = event_loop
        .run(None, &mut handler, |handler| {
            if !handler.running {
                signal.stop();
            }
        })
        .context(CalloopSnafu {
            thing: "event loop failed",
        });
    handler.window.join()?;
    result.map(|()| handler.outcome)
}

impl Handler {
    fn on_agent_event(&mut self, event: &AgentEvent, stream: UnixStream) {
        if permission::settles(&event.kind) {
            self.settle_prompts(&(event.agent, event.session_id.clone()));
        }
        let animation = self.state.on_event(event, Instant::now());
        self.show(animation);
        if let Some(pid) = event.agent_pid.or(event.source_pid) {
            self.watch(pid);
        }
        if matches!(event.kind, EventKind::PermissionRequest { .. }) {
            self.open_prompt(event, stream);
        }
    }

    fn watch(&mut self, pid: u32) {
        if !self.watched.insert(pid) {
            return;
        }
        match self.on_exit(pid, move |handler| handler.on_process_exit(pid)) {
            Watch::Watching(_) => {}
            Watch::AlreadyExited => self.on_process_exit(pid),
            Watch::Unwatchable => {
                self.watched.remove(&pid);
            }
        }
    }

    fn on_exit(&mut self, pid: u32, exited: impl FnOnce(&mut Self) + 'static) -> Watch {
        let pidfd = Pid::from_raw(pid as i32)
            .ok_or(Errno::SRCH)
            .and_then(|process| pidfd_open(process, PidfdFlags::empty()));
        let pidfd = match pidfd {
            Ok(pidfd) => pidfd,
            Err(Errno::SRCH) => return Watch::AlreadyExited,
            Err(error) => {
                eprintln!("Unable to watch process {pid}: {error}");
                return Watch::Unwatchable;
            }
        };
        let mut exited = Some(exited);
        let inserted = self.handle.insert_source(
            Generic::new(pidfd, Interest::READ, Mode::Level),
            move |_, _, handler: &mut Self| {
                if let Some(exited) = exited.take() {
                    exited(handler);
                }
                Ok(PostAction::Remove)
            },
        );
        match inserted {
            Ok(token) => Watch::Watching(token),
            Err(error) => {
                self.fail(HandlerError::Calloop {
                    source: error.error,
                    thing: format!("watching process {pid}"),
                });
                Watch::Unwatchable
            }
        }
    }

    fn open_prompt(&mut self, event: &AgentEvent, hook: UnixStream) {
        let prompt = match Prompt::open(event, hook) {
            Ok(prompt) => prompt,
            Err(error) => {
                eprintln!("{}", snafu::Report::from_error(error));
                return;
            }
        };
        let id = self.next_prompt;
        self.next_prompt += 1;
        let prompt_pid = prompt.prompt_pid();
        let hook_pid = prompt.hook_pid();
        self.prompts.insert(id, prompt);
        match self.on_exit(prompt_pid, move |handler| handler.answer_prompt(id)) {
            Watch::Watching(token) => self.add_prompt_watch(id, token),
            Watch::AlreadyExited => return self.answer_prompt(id),
            Watch::Unwatchable => return self.close_prompt(id),
        }
        let hook_pid = match hook_pid {
            Ok(pid) => pid,
            Err(error) => {
                eprintln!("{}", snafu::Report::from_error(error));
                return;
            }
        };
        match self.on_exit(hook_pid, move |handler| handler.close_prompt(id)) {
            Watch::Watching(token) => self.add_prompt_watch(id, token),
            Watch::AlreadyExited => self.close_prompt(id),
            Watch::Unwatchable => {}
        }
    }

    fn open_settings(&mut self) {
        let spawned = get_sibling_executable(GUI_BINARY)
            .map_err(|error| snafu::Report::from_error(error).to_string())
            .and_then(|program| {
                Command::new(&program)
                    .args([SETTINGS_MODE, DISPLAYS_PAGE])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
                    .map_err(|error| format!("Unable to start {}: {error}", program.display()))
            });
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };
        let pid = child.id();
        self.settings.insert(pid, child);
        let watch = self.on_exit(pid, move |handler| handler.reap_settings(pid));
        if matches!(watch, Watch::AlreadyExited) {
            self.reap_settings(pid);
        }
    }

    fn reap_settings(&mut self, pid: u32) {
        if let Some(mut child) = self.settings.remove(&pid) {
            let _ = child.wait();
        }
    }

    fn add_prompt_watch(&mut self, id: u64, token: RegistrationToken) {
        if let Some(prompt) = self.prompts.get_mut(&id) {
            prompt.watches.push(token);
        }
    }

    fn answer_prompt(&mut self, id: u64) {
        if let Some(prompt) = self.remove_prompt(id)
            && let Err(error) = prompt.answer()
        {
            eprintln!("{}", snafu::Report::from_error(error));
        }
    }

    fn close_prompt(&mut self, id: u64) {
        self.remove_prompt(id);
    }

    fn settle_prompts(&mut self, session: &(Agent, String)) {
        let settled: Vec<u64> = self
            .prompts
            .iter()
            .filter(|(_, prompt)| &prompt.session == session)
            .map(|(id, _)| *id)
            .collect();
        for id in settled {
            self.close_prompt(id);
        }
    }

    fn remove_prompt(&mut self, id: u64) -> Option<Prompt> {
        let prompt = self.prompts.remove(&id)?;
        for token in &prompt.watches {
            self.handle.remove(*token);
        }
        Some(prompt)
    }

    fn on_process_exit(&mut self, pid: u32) {
        self.watched.remove(&pid);
        let animation = self.state.on_process_exit(pid, Instant::now());
        self.show(animation);
    }

    fn fail(&mut self, error: HandlerError) {
        eprintln!("{}", snafu::Report::from_error(error));
        self.running = false;
    }

    fn on_click(&mut self, side: Side) {
        let now = Instant::now();
        let animation = self
            .clicks
            .click(side, now)
            .and_then(|gesture| self.state.react(gesture, roll(), now));
        self.show(animation);
    }

    fn on_deadline(&mut self) {
        self.timer = None;
        let now = Instant::now();
        let reaction = self
            .clicks
            .expire(now)
            .and_then(|gesture| self.state.react(gesture, roll(), now));
        let animation = self.state.on_deadline(now, roll()).or(reaction);
        self.show(animation);
    }

    fn show(&mut self, animation: Option<Animations>) {
        if let Some(animation) = animation {
            match self.window.set_animation(animation) {
                Ok(()) | Err(WindowError::Missing { .. }) => {}
                Err(_) => self.running = false,
            }
        }
        let roaming = self.state.is_roaming();
        if roaming != self.roaming {
            self.roaming = roaming;
            if self.window.set_roaming(roaming).is_err() {
                self.running = false;
            }
        }
        self.track_cursor();
        self.reschedule();
    }

    fn wants_cursor(&self) -> bool {
        !self.cursor.unavailable && (self.roaming || self.state.dizzy_armed())
    }

    fn track_cursor(&mut self) {
        if self.cursor.polling || !self.wants_cursor() {
            return;
        }
        let inserted = self
            .handle
            .insert_source(Timer::immediate(), |_, (), handler| handler.poll_cursor());
        match inserted {
            Ok(_) => self.cursor.polling = true,
            Err(error) => self.fail(HandlerError::Calloop {
                source: error.error,
                thing: "polling the cursor".to_owned(),
            }),
        }
    }

    fn poll_cursor(&mut self) -> TimeoutAction {
        if !self.wants_cursor() {
            self.stop_cursor();
            return TimeoutAction::Drop;
        }
        let position = match CursorPosition::get() {
            Ok(position) => (position.x, position.y),
            Err(error) => {
                eprintln!("Cursor tracking disabled: {error}");
                self.cursor.unavailable = true;
                self.stop_cursor();
                return TimeoutAction::Drop;
            }
        };
        let moved = self.cursor.last.is_some_and(|last| last != position);
        self.cursor.last = Some(position);
        if moved {
            self.cursor_moved(position);
        }
        TimeoutAction::ToDuration(CURSOR_POLL)
    }

    const fn stop_cursor(&mut self) {
        self.cursor.polling = false;
        self.cursor.last = None;
        self.cursor.spin.reset();
    }

    fn cursor_moved(&mut self, (x, y): (i64, i64)) {
        let now = Instant::now();
        if self.roaming {
            let animation = self.state.end_roam(now);
            self.show(animation);
            return;
        }
        let Some((center_x, center_y)) = self.cursor.center else {
            return;
        };
        let offset = (
            f64::from(x as i32) - center_x,
            f64::from(y as i32) - center_y,
        );
        if self.cursor.spin.moved(offset, now) {
            let animation = self.state.dizzy(now);
            self.show(animation);
        }
    }

    fn reschedule(&mut self) {
        let deadline = match (self.state.deadline(), self.clicks.deadline()) {
            (Some(state), Some(clicks)) => Some(state.min(clicks)),
            (state, clicks) => state.or(clicks),
        };
        if self.timer.as_ref().map(|(at, _)| *at) == deadline {
            return;
        }
        if let Some((_, token)) = self.timer.take() {
            self.handle.remove(token);
        }
        let Some(at) = deadline else {
            return;
        };
        match self
            .handle
            .insert_source(Timer::from_deadline(at), |_, (), handler| {
                handler.on_deadline();
                TimeoutAction::Drop
            }) {
            Ok(token) => self.timer = Some((at, token)),
            Err(error) => self.fail(HandlerError::Calloop {
                source: error.error,
                thing: "scheduling a state timer".to_owned(),
            }),
        }
    }
}
