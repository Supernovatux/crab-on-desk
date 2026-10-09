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
    sync::Arc,
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, LoopHandle, RegistrationToken,
    channel::{self, Channel, Sender},
    timer::{TimeoutAction, Timer},
};
use crab_common::atlas::{Animations, Rect, TrackingLayer};
use snafu::ResultExt;

use super::{CalloopSnafu, Side, Window, WindowCommand, WindowError, WindowEvent};
use crate::{
    placement::{Area, Location},
    random::roll,
    renderer::{LayerOffset, Renderer},
    theme::Theme,
};

const SNAP_TOLERANCE: i32 = 30;
const PEEK_OFFSET: i32 = 25;
const UNDOCK_DISTANCE: i32 = 100;
const ROAM_MARGIN: f64 = 0.15;
const ROAM_MIN_DISTANCE: f64 = 100.0;
const ROAM_PX_PER_MS: f64 = 0.08;
const ROAM_MIN_DURATION_MS: f64 = 1000.0;
const ROAM_ATTEMPTS: usize = 8;
const ROAM_STEP: Duration = Duration::from_millis(16);
const DRAG_THRESHOLD: f64 = 3.0;
const LOOK_STEP: Duration = Duration::from_millis(16);
const LOOK_FULL_DISTANCE: f64 = 300.0;
const LOOK_LIMIT: (f64, f64) = (0.85, 0.5);
const LOOK_SETTLED: f64 = 0.002;
const OFFSET_STEPS_PER_PIXEL: f64 = 4.0;

pub type Position = (i32, i32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: Option<String>,
    pub area: Area,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Default,
    Grabbing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputChange {
    Unchanged,
    Moved,
    Remapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Enter,
    Leave,
    Motion((f64, f64)),
    Press(Button, (f64, f64)),
    Release(Button, (f64, f64)),
}

pub trait Surface: Sized + 'static {
    fn output(&self) -> Option<Output>;
    fn origin(&self) -> Option<Position>;
    fn target(&self) -> Option<Position>;
    fn place(&mut self, home: Position);
    fn move_to(&mut self, target: Position) -> bool;
    fn set_input_region(&mut self, region: Area) -> Result<(), WindowError>;
    fn set_cursor(&mut self, shape: CursorShape);
    fn move_to_output<R: Renderer>(
        &mut self,
        name: &str,
        renderer: &mut R,
    ) -> Result<OutputChange, WindowError>;
    fn present<R: Renderer>(&mut self, renderer: &R) -> Result<bool, WindowError>;
}

pub struct Widget<S: Surface, R: Renderer + 'static> {
    pub surface: S,
    renderer: Box<R>,
    loop_handle: LoopHandle<'static, Self>,
    events: Sender<WindowEvent>,
    theme: Arc<Theme>,
    size: (u32, u32),
    gesture: Gesture,
    dock: Dock,
    walk: Option<Walk>,
    heading_left: bool,
    requested: Animations,
    animation: Option<Animations>,
    hitbox: Option<(Rect, u32, u32)>,
    frame_delays: Vec<Duration>,
    loops: bool,
    frame: usize,
    frame_timer: Option<RegistrationToken>,
    look: Look,
    presentation: Presentation,
    exit: bool,
    error: Option<WindowError>,
}

pub struct Running<S: Surface, R: Renderer + 'static> {
    pub widget: Widget<S, R>,
    pub event_loop: EventLoop<'static, Widget<S, R>>,
}

impl<S: Surface, R: Renderer + 'static> Window for Running<S, R> {
    fn run(
        self: Box<Self>,
        animation: Animations,
        commands: Channel<WindowCommand>,
    ) -> Result<(), WindowError> {
        let Self { widget, event_loop } = *self;
        widget.run(event_loop, animation, commands)
    }
}

#[derive(Default)]
struct Look {
    cursor: Option<(f64, f64)>,
    anchor: Option<(f64, f64)>,
    layers: Vec<LookLayer>,
    timer: Option<RegistrationToken>,
}

struct LookLayer {
    tracking: TrackingLayer,
    direction: (f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Released,
    Pressed { at: (f64, f64) },
    Dragging { grab: (f64, f64) },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dock {
    Free,
    Docked {
        side: Side,
        restore: Position,
        x: i32,
        peeking: bool,
    },
}

struct Walk {
    from: Position,
    to: Position,
    start: Instant,
    duration: Duration,
    timer: RegistrationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presentation {
    Unmapped,
    Idle,
    AwaitingFrame,
    AwaitingFrameDirty,
}

impl<S: Surface, R: Renderer + 'static> Widget<S, R> {
    pub fn new(
        surface: S,
        renderer: Box<R>,
        size: (u32, u32),
        theme: Arc<Theme>,
        events: Sender<WindowEvent>,
        loop_handle: LoopHandle<'static, Self>,
    ) -> Self {
        Self {
            surface,
            renderer,
            loop_handle,
            events,
            theme,
            size,
            gesture: Gesture::Released,
            dock: Dock::Free,
            walk: None,
            heading_left: false,
            requested: Animations::default(),
            animation: None,
            hitbox: None,
            frame_delays: Vec::new(),
            loops: true,
            frame: 0,
            frame_timer: None,
            look: Look::default(),
            presentation: Presentation::Unmapped,
            exit: false,
            error: None,
        }
    }

    fn run(
        mut self,
        mut event_loop: EventLoop<'static, Self>,
        animation: Animations,
        commands: Channel<WindowCommand>,
    ) -> Result<(), WindowError> {
        self.requested = animation;
        self.show()?;
        event_loop
            .handle()
            .insert_source(commands, |event, (), widget| {
                let result = match event {
                    channel::Event::Msg(WindowCommand::Show(animation)) => {
                        widget.requested = animation;
                        widget.show()
                    }
                    channel::Event::Msg(WindowCommand::Roam(true)) => widget.start_walk(),
                    channel::Event::Msg(WindowCommand::Roam(false)) => {
                        widget.stop_walk(false);
                        Ok(())
                    }
                    channel::Event::Msg(WindowCommand::MoveToOutput(name)) => {
                        widget.move_to_output(&name)
                    }
                    channel::Event::Msg(WindowCommand::Look(cursor)) => {
                        widget.look.cursor = cursor;
                        widget.follow_cursor()
                    }
                    channel::Event::Closed => {
                        widget.exit = true;
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    widget.fail(e);
                }
            })
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding command channel",
            })?;
        let loop_signal = event_loop.get_signal();
        event_loop
            .run(None, &mut self, move |widget| {
                if widget.exit {
                    loop_signal.stop();
                }
            })
            .context(CalloopSnafu {
                thing: "event loop failed",
            })?;
        self.error.take().map_or(Ok(()), Err)
    }

    pub fn send(&self, event: WindowEvent) {
        let _ = self.events.send(event);
    }

    #[cfg(feature = "wayland")]
    pub const fn close(&mut self) {
        self.exit = true;
    }

    pub fn fail(&mut self, error: impl Into<WindowError>) {
        self.error = Some(error.into());
        self.exit = true;
    }

    pub fn idle_timeout(&self) -> Duration {
        let sleep = self.theme.behaviour().sleep;
        Duration::from_millis(u64::from(sleep.idle_after_ms.min(sleep.yawn_after_ms)))
    }

    pub fn pointer(&mut self, event: Pointer) {
        if let Err(e) = self.pointer_event(event) {
            self.fail(e);
        }
    }

    #[cfg(feature = "wayland")]
    pub const fn pointer_lost(&mut self) {
        self.gesture = Gesture::Released;
    }

    pub fn moved(&mut self) {
        self.report_location();
        if let Err(e) = self.follow_cursor() {
            self.fail(e);
        }
    }

    #[cfg(feature = "wayland")]
    pub fn resized(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        let resized = self
            .renderer
            .resize(size.0, size.1)
            .map_err(WindowError::from)
            .and_then(|()| self.update_input_region());
        if let Err(e) = resized {
            self.fail(e);
            return;
        }
        self.invalidate();
    }

    #[cfg(feature = "wayland")]
    pub fn mapped(&mut self) {
        if self.presentation == Presentation::Unmapped {
            self.redraw();
        }
    }

    #[cfg(feature = "x11")]
    pub fn exposed(&mut self) {
        if self.presentation == Presentation::Unmapped {
            self.redraw();
        } else {
            self.invalidate();
        }
    }

    #[cfg(feature = "wayland")]
    pub fn frame_done(&mut self) {
        if self.presentation == Presentation::AwaitingFrameDirty {
            self.redraw();
        } else {
            self.presentation = Presentation::Idle;
        }
    }

    pub fn report_location(&self) {
        if let Some(location) = self.location() {
            self.send(WindowEvent::Moved(location));
        }
    }

    fn show(&mut self) -> Result<(), WindowError> {
        let animation = match self.gesture {
            Gesture::Dragging { .. } if self.theme.contains(Animations::ReactDrag) => {
                Animations::ReactDrag
            }
            _ => self.requested,
        };
        if self.animation == Some(animation) {
            return Ok(());
        }
        let loaded = self.theme.animation(animation)?;
        self.renderer.set_animation(&loaded)?;
        self.animation = Some(animation);
        self.hitbox = Some((loaded.hitbox, loaded.width, loaded.height));
        self.update_input_region()?;
        self.look.anchor = loaded.anchor;
        self.look.layers = loaded
            .layers
            .iter()
            .map(|layer| LookLayer {
                tracking: layer.tracking,
                direction: (0.0, 0.0),
            })
            .collect();
        self.follow_cursor()?;
        self.frame_delays = loaded.frame_delays;
        self.loops = loaded.loops;
        self.frame = 0;
        if let Some(token) = self.frame_timer.take() {
            self.loop_handle.remove(token);
        }
        if let Some(delay) = self.frame_delays.first() {
            let token = self
                .loop_handle
                .insert_source(Timer::from_duration(*delay), |deadline, (), widget| {
                    widget.advance_frame(deadline)
                })
                .map_err(|e| e.error)
                .context(CalloopSnafu {
                    thing: "adding frame timer",
                })?;
            self.frame_timer = Some(token);
        }
        self.invalidate();
        Ok(())
    }

    fn update_input_region(&mut self) -> Result<(), WindowError> {
        let Some((hitbox, width, height)) = self.hitbox else {
            return Ok(());
        };
        let (x, y, w, h) = input_rect(hitbox, (width, height), self.size);
        let x = if self.mirrored() {
            self.size.0 as i32 - x - w
        } else {
            x
        };
        self.surface.set_input_region(Area {
            x,
            y,
            width: w,
            height: h,
        })
    }

    fn pointer_event(&mut self, event: Pointer) -> Result<(), WindowError> {
        match event {
            Pointer::Enter => {
                self.surface.set_cursor(CursorShape::Default);
                self.peek(true);
            }
            Pointer::Leave => {
                self.end_gesture()?;
                self.peek(false);
            }
            Pointer::Motion(position) => self.pointer_moved(position)?,
            Pointer::Press(Button::Left, at) => self.gesture = Gesture::Pressed { at },
            Pointer::Press(Button::Right, _) => {
                if matches!(self.gesture, Gesture::Released) {
                    self.send(WindowEvent::OpenSettings);
                }
            }
            Pointer::Release(Button::Left, position) => {
                if matches!(self.gesture, Gesture::Pressed { .. }) {
                    if matches!(self.dock, Dock::Docked { .. }) {
                        self.undock()?;
                    } else {
                        let side = if position.0 < f64::from(self.size.0) / 2.0 {
                            Side::Left
                        } else {
                            Side::Right
                        };
                        self.send(WindowEvent::Click { side });
                    }
                }
                self.end_gesture()?;
            }
            Pointer::Release(Button::Right, _) => {}
        }
        Ok(())
    }

    fn pointer_moved(&mut self, position: (f64, f64)) -> Result<(), WindowError> {
        let grab = match self.gesture {
            Gesture::Released => return Ok(()),
            Gesture::Dragging { grab } => grab,
            Gesture::Pressed { .. } if matches!(self.dock, Dock::Docked { .. }) => return Ok(()),
            Gesture::Pressed { at } => {
                if (position.0 - at.0).abs() <= DRAG_THRESHOLD
                    && (position.1 - at.1).abs() <= DRAG_THRESHOLD
                {
                    return Ok(());
                }
                if !self.place() {
                    return Ok(());
                }
                self.stop_walk(true);
                self.gesture = Gesture::Dragging { grab: at };
                self.surface.set_cursor(CursorShape::Grabbing);
                self.show()?;
                at
            }
        };
        let Some(origin) = self.origin() else {
            return Ok(());
        };
        let target = self.clamp((
            origin.0 + (position.0 - grab.0).round() as i32,
            origin.1 + (position.1 - grab.1).round() as i32,
        ));
        self.move_to(target);
        Ok(())
    }

    fn end_gesture(&mut self) -> Result<(), WindowError> {
        let was_dragging = matches!(self.gesture, Gesture::Dragging { .. });
        self.gesture = Gesture::Released;
        if was_dragging {
            self.surface.set_cursor(CursorShape::Default);
            self.show()?;
            self.try_dock()?;
        }
        Ok(())
    }

    fn snap_zone(&self, side: Side) -> Option<i32> {
        let output = self.surface.output()?;
        let width = self.size.0 as i32;
        let overhang = width / 4;
        Some(match side {
            Side::Left => -overhang + SNAP_TOLERANCE,
            Side::Right => output.area.width - width + overhang - SNAP_TOLERANCE,
        })
    }

    fn in_snap_zone(&self, side: Side, x: i32) -> bool {
        self.snap_zone(side).is_some_and(|zone| match side {
            Side::Left => x <= zone,
            Side::Right => x >= zone,
        })
    }

    fn try_dock(&mut self) -> Result<(), WindowError> {
        let (Some(mini), Some(target), Some(output)) = (
            self.theme.behaviour().mini,
            self.surface.target(),
            self.surface.output(),
        ) else {
            return Ok(());
        };
        if !self.theme.contains(Animations::MiniIdle) {
            return Ok(());
        }
        let side = if self.in_snap_zone(Side::Right, target.0) {
            Side::Right
        } else if self.in_snap_zone(Side::Left, target.0) {
            Side::Left
        } else {
            return Ok(());
        };
        let width = f64::from(self.size.0);
        let x = match side {
            Side::Left => -(width * mini.offset_ratio).round() as i32,
            Side::Right => output.area.width - (width * (1.0 - mini.offset_ratio)).round() as i32,
        };
        self.dock = Dock::Docked {
            side,
            restore: target,
            x,
            peeking: false,
        };
        self.move_to((x, target.1));
        self.send(WindowEvent::Docked);
        self.refresh_orientation()
    }

    fn peek(&mut self, inside: bool) {
        let (
            Dock::Docked {
                side,
                restore,
                x,
                peeking,
            },
            Some(target),
        ) = (self.dock, self.surface.target())
        else {
            return;
        };
        if peeking == inside {
            return;
        }
        self.dock = Dock::Docked {
            side,
            restore,
            x,
            peeking: inside,
        };
        let offset = match (inside, side) {
            (false, _) => 0,
            (true, Side::Left) => PEEK_OFFSET,
            (true, Side::Right) => -PEEK_OFFSET,
        };
        self.move_to((x + offset, target.1));
        self.send(WindowEvent::Hover { inside });
    }

    fn undock(&mut self) -> Result<(), WindowError> {
        let Dock::Docked { side, restore, .. } = self.dock else {
            return Ok(());
        };
        self.dock = Dock::Free;
        let x = match (side, self.snap_zone(side)) {
            (Side::Left, Some(zone)) if restore.0 <= zone => zone + UNDOCK_DISTANCE,
            (Side::Right, Some(zone)) if restore.0 >= zone => {
                zone + SNAP_TOLERANCE - UNDOCK_DISTANCE
            }
            _ => restore.0,
        };
        self.move_to((x, restore.1));
        self.send(WindowEvent::Undocked);
        self.refresh_orientation()
    }

    fn mirrored(&self) -> bool {
        let docked_left = matches!(
            self.dock,
            Dock::Docked {
                side: Side::Left,
                ..
            }
        );
        let flip_assets = self
            .theme
            .behaviour()
            .mini
            .is_some_and(|mini| mini.flip_assets);
        match self.animation {
            Some(Animations::Roam) => self.heading_left != self.theme.behaviour().roam_flip_assets,
            Some(animation) if animation.is_mini() => docked_left != flip_assets,
            _ => false,
        }
    }

    fn start_walk(&mut self) -> Result<(), WindowError> {
        let walkable = self.walk.is_none()
            && self.dock == Dock::Free
            && self.gesture == Gesture::Released
            && self.place();
        let Some((from, to)) = self
            .surface
            .target()
            .filter(|_| walkable)
            .and_then(|from| Some((from, self.roam_target(from)?)))
        else {
            self.send(WindowEvent::RoamEnded);
            return Ok(());
        };
        let distance = f64::from(to.0 - from.0).hypot(f64::from(to.1 - from.1));
        let duration_ms = (distance / ROAM_PX_PER_MS).max(ROAM_MIN_DURATION_MS);
        let timer = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, (), widget| widget.walk_step())
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding roam timer",
            })?;
        if to.0 != from.0 {
            self.heading_left = to.0 < from.0;
        }
        self.walk = Some(Walk {
            from,
            to,
            start: Instant::now(),
            duration: Duration::from_millis(duration_ms as u64),
            timer,
        });
        self.refresh_orientation()
    }

    fn roam_target(&self, from: Position) -> Option<Position> {
        let area = self.surface.output()?.area;
        let margin_x = (f64::from(area.width) * ROAM_MARGIN).round() as i32;
        let margin_y = (f64::from(area.height) * ROAM_MARGIN).round() as i32;
        let x_range = (margin_x, area.width - self.size.0 as i32 - margin_x);
        let y_range = (margin_y, area.height - self.size.1 as i32 - margin_y);
        if x_range.1 < x_range.0 || y_range.1 < y_range.0 {
            return None;
        }
        let pick = |(low, high): (i32, i32)| {
            let span = u64::from((high - low).unsigned_abs()) + 1;
            low + (roll() % span) as i32
        };
        let candidates: Vec<Position> = (0..ROAM_ATTEMPTS)
            .map(|_| (pick(x_range), pick(y_range)))
            .collect();
        let far_enough = |to: &&Position| {
            f64::from(to.0 - from.0).hypot(f64::from(to.1 - from.1)) >= ROAM_MIN_DISTANCE
        };
        candidates
            .iter()
            .find(far_enough)
            .or_else(|| candidates.last())
            .copied()
    }

    fn walk_step(&mut self) -> TimeoutAction {
        let Some(walk) = &self.walk else {
            return TimeoutAction::Drop;
        };
        let progress = (walk.start.elapsed().as_secs_f64() / walk.duration.as_secs_f64()).min(1.0);
        let eased = progress * (2.0 - progress);
        let along = |from: i32, to: i32| {
            (f64::from(to - from).mul_add(eased, f64::from(from))).round() as i32
        };
        let position = (along(walk.from.0, walk.to.0), along(walk.from.1, walk.to.1));
        self.move_to(position);
        if progress < 1.0 {
            return TimeoutAction::ToDuration(ROAM_STEP);
        }
        self.walk = None;
        self.send(WindowEvent::RoamEnded);
        TimeoutAction::Drop
    }

    fn stop_walk(&mut self, notify: bool) {
        if let Some(walk) = self.walk.take() {
            self.loop_handle.remove(walk.timer);
            if notify {
                self.send(WindowEvent::RoamEnded);
            }
        }
    }

    fn home(&self) -> Option<Position> {
        let area = self.surface.output()?.area;
        Some((
            area.width - self.size.0 as i32,
            (area.height - self.size.1 as i32) / 2,
        ))
    }

    fn origin(&self) -> Option<Position> {
        self.surface.origin().or_else(|| self.home())
    }

    fn location(&self) -> Option<Location> {
        let Output { name, area } = self.surface.output()?;
        let (x, y) = self.origin()?;
        Some(Location {
            output_name: name,
            output: area,
            crab: Area {
                x: area.x + x,
                y: area.y + y,
                width: self.size.0 as i32,
                height: self.size.1 as i32,
            },
        })
    }

    fn refresh_orientation(&mut self) -> Result<(), WindowError> {
        self.update_input_region()?;
        self.invalidate();
        self.follow_cursor()
    }

    fn look_direction(&self) -> (f64, f64) {
        let (
            Some((cursor_x, cursor_y)),
            Some((anchor_x, anchor_y)),
            Some(location),
            Some((_, width, height)),
        ) = (
            self.look.cursor,
            self.look.anchor,
            self.location(),
            self.hitbox,
        )
        else {
            return (0.0, 0.0);
        };
        let (origin_x, origin_y) = (f64::from(location.crab.x), f64::from(location.crab.y));
        let mirrored = self.mirrored();
        let (scale, left, top) = fit((width, height), self.size);
        let anchor_x = if mirrored {
            f64::from(width) - anchor_x
        } else {
            anchor_x
        };
        let relative_x = cursor_x - anchor_x.mul_add(scale, origin_x + left);
        let relative_y = cursor_y - anchor_y.mul_add(scale, origin_y + top);
        let distance = relative_x.hypot(relative_y);
        if distance <= 1.0 {
            return (0.0, 0.0);
        }
        let reach = (distance / LOOK_FULL_DISTANCE).min(1.0) / distance;
        let x = (relative_x * reach).clamp(-LOOK_LIMIT.0, LOOK_LIMIT.0);
        let y = (relative_y * reach).clamp(-LOOK_LIMIT.1, LOOK_LIMIT.1);
        (if mirrored { -x } else { x }, y)
    }

    fn layer_offsets(&self) -> Vec<LayerOffset> {
        self.look
            .layers
            .iter()
            .map(|layer| {
                let [max_x, max_y] = layer.tracking.max_offset;
                let (x, y) = layer.direction;
                LayerOffset {
                    x: quantize(x * max_x),
                    y: quantize(y * max_y),
                    stretch_x: x.abs().mul_add(layer.tracking.stretch_x, 1.0),
                }
            })
            .collect()
    }

    fn look_settled(&self) -> bool {
        let target = self.look_direction();
        self.look
            .layers
            .iter()
            .filter(|layer| layer.tracking != TrackingLayer::default())
            .all(|layer| layer.direction == target)
    }

    fn follow_cursor(&mut self) -> Result<(), WindowError> {
        if self.look.timer.is_some() || self.look_settled() {
            return Ok(());
        }
        let token = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, (), widget| widget.look_step())
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding look timer",
            })?;
        self.look.timer = Some(token);
        Ok(())
    }

    fn look_step(&mut self) -> TimeoutAction {
        let (target_x, target_y) = self.look_direction();
        let before = self.layer_offsets();
        let approach = |current: f64, target: f64, ease: f64| {
            let next = (target - current).mul_add(ease, current);
            if (target - next).abs() < LOOK_SETTLED {
                target
            } else {
                next
            }
        };
        for layer in &mut self.look.layers {
            let ease = layer.tracking.ease;
            let (x, y) = layer.direction;
            layer.direction = (approach(x, target_x, ease), approach(y, target_y, ease));
        }
        if self.layer_offsets() != before {
            self.invalidate();
        }
        if self.look_settled() {
            self.look.timer = None;
            return TimeoutAction::Drop;
        }
        TimeoutAction::ToDuration(LOOK_STEP)
    }

    fn move_to(&mut self, target: Position) {
        if self.surface.move_to(target) {
            self.moved();
        }
    }

    fn move_to_output(&mut self, name: &str) -> Result<(), WindowError> {
        let change = self.surface.move_to_output(name, &mut *self.renderer)?;
        if change == OutputChange::Unchanged {
            return Ok(());
        }
        self.stop_walk(true);
        self.gesture = Gesture::Released;
        if change == OutputChange::Remapped {
            self.presentation = Presentation::Unmapped;
        }
        if matches!(self.dock, Dock::Docked { .. }) {
            self.dock = Dock::Free;
            self.send(WindowEvent::Undocked);
        }
        if change == OutputChange::Moved
            && let Some(home) = self.home()
        {
            self.move_to(home);
        }
        self.update_input_region()?;
        self.report_location();
        Ok(())
    }

    fn place(&mut self) -> bool {
        let Some(home) = self.home() else {
            return false;
        };
        self.surface.place(home);
        true
    }

    fn clamp(&self, (x, y): Position) -> Position {
        let (output_width, output_height) = self
            .surface
            .output()
            .map_or((i32::MAX, i32::MAX), |output| {
                (output.area.width, output.area.height)
            });
        let (width, height) = (self.size.0 as i32, self.size.1 as i32);
        let overhang = width / 4;
        let max_x = (output_width - width + overhang).max(-overhang);
        let max_y = (output_height - height).max(0);
        (x.clamp(-overhang, max_x), y.clamp(0, max_y))
    }

    fn advance_frame(&mut self, deadline: Instant) -> TimeoutAction {
        let next = self.frame + 1;
        if next < self.frame_delays.len() {
            self.frame = next;
        } else if self.loops {
            self.frame = 0;
        } else {
            self.frame_timer = None;
            return TimeoutAction::Drop;
        }
        self.invalidate();
        self.frame_delays
            .get(self.frame)
            .map_or(TimeoutAction::Drop, |delay| {
                TimeoutAction::ToInstant(deadline + *delay)
            })
    }

    fn invalidate(&mut self) {
        match self.presentation {
            Presentation::Idle => self.redraw(),
            Presentation::AwaitingFrame => self.presentation = Presentation::AwaitingFrameDirty,
            Presentation::Unmapped | Presentation::AwaitingFrameDirty => {}
        }
    }

    fn redraw(&mut self) {
        if let Err(e) = self.draw() {
            self.fail(e);
        }
    }

    fn draw(&mut self) -> Result<(), WindowError> {
        self.presentation = Presentation::AwaitingFrame;
        let mirrored = self.mirrored();
        let offsets = self.layer_offsets();
        self.renderer.draw(
            self.size.0 as i32,
            self.size.1 as i32,
            self.frame as u32,
            mirrored,
            &offsets,
        );
        if !self.surface.present(&*self.renderer)? {
            self.presentation = Presentation::Idle;
        }
        Ok(())
    }
}

fn quantize(offset: f64) -> f64 {
    (offset * OFFSET_STEPS_PER_PIXEL).round() / OFFSET_STEPS_PER_PIXEL
}

fn fit(content: (u32, u32), surface: (u32, u32)) -> (f64, f64, f64) {
    let (content_width, content_height) = (f64::from(content.0), f64::from(content.1));
    let (surface_width, surface_height) = (f64::from(surface.0), f64::from(surface.1));
    let scale = (surface_width / content_width).min(surface_height / content_height);
    let left = (content_width.mul_add(-scale, surface_width)) / 2.0;
    let top = (content_height.mul_add(-scale, surface_height)) / 2.0;
    (scale, left, top)
}

fn input_rect(hitbox: Rect, content: (u32, u32), surface: (u32, u32)) -> (i32, i32, i32, i32) {
    let (scale, left, top) = fit(content, surface);
    let x0 = f64::from(hitbox.x).mul_add(scale, left).floor();
    let y0 = f64::from(hitbox.y).mul_add(scale, top).floor();
    let x1 = f64::from(hitbox.x + hitbox.width)
        .mul_add(scale, left)
        .ceil();
    let y1 = f64::from(hitbox.y + hitbox.height)
        .mul_add(scale, top)
        .ceil();
    (x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32)
}
