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
    ptr::NonNull,
    sync::Arc,
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, LoopHandle, RegistrationToken,
    channel::{self, Channel, Sender},
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use crab_common::atlas::{Animations, Rect};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    error::GlobalError as SctkGlobalError,
    output::{OutputHandler, OutputState},
    reexports::protocols::{
        ext::idle_notify::v1::client::{
            ext_idle_notification_v1::{self, ExtIdleNotificationV1},
            ext_idle_notifier_v1::ExtIdleNotifierV1,
        },
        wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatError, SeatHandler, SeatState,
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use snafu::{ResultExt, Snafu};
use wayland_client::{
    ConnectError, Connection, DispatchError, Proxy, QueueHandle,
    globals::{BindError, GlobalError, registry_queue_init},
    protocol::{wl_callback, wl_output, wl_pointer, wl_seat, wl_surface},
};
use wayland_egl::WlEglSurface;

use super::{Side, WindowCommand, WindowEvent};
use crate::random::roll;
use crate::{
    renderer::{Renderer, RendererError},
    theme::{Theme, ThemeError},
};

const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const INPUT_IDLE_VERSION: u32 = 2;
const NAMESPACE: &str = "carb-on-desk";
const SIZE: u32 = 200;
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

#[derive(Debug, Snafu)]
pub enum WaylandError {
    #[snafu(display("Wayland connection error"))]
    #[snafu(context(false))]
    Connection { source: ConnectError },
    #[snafu(display("Wayland global error"))]
    #[snafu(context(false))]
    Global { source: GlobalError },
    #[snafu(display("{thing} not available."))]
    Bind { source: BindError, thing: String },
    #[snafu(display("Display handle not available."))]
    Display,
    #[snafu(display("Error during dispatch"))]
    #[snafu(context(false))]
    Dispatch { source: DispatchError },
    #[snafu(display("Window handle not available."))]
    Window,
    #[snafu(display("Wayland error while {thing}."))]
    Wayland {
        source: wayland_egl::Error,
        thing: String,
    },
    #[snafu(display("Renderer error"))]
    #[snafu(context(false))]
    Renderer { source: RendererError },
    #[snafu(display("Theme error"))]
    #[snafu(context(false))]
    Theme { source: ThemeError },
    #[snafu(display("Unable to create the input region"))]
    #[snafu(context(false))]
    Region { source: SctkGlobalError },
    #[snafu(display("Unable to use the pointer"))]
    #[snafu(context(false))]
    Seat { source: SeatError },
    #[snafu(display("Calloop Error at {thing}"))]
    Calloop {
        source: calloop::Error,
        thing: String,
    },
}

pub struct WaylandWindow<T: Renderer + 'static> {
    window: _WaylandWindow<T>,
    event_loop: EventLoop<'static, _WaylandWindow<T>>,
}
impl<T: Renderer + 'static> super::Window for WaylandWindow<T> {
    fn new(
        theme: Arc<Theme>,
        events: Sender<WindowEvent>,
    ) -> Result<Box<Self>, super::WindowError> {
        let (window, event_loop) = _WaylandWindow::new(theme, events)?;
        Ok(Box::new(Self { window, event_loop }))
    }
    fn run(
        mut self: Box<Self>,
        animation: Animations,
        commands: Channel<WindowCommand>,
    ) -> Result<(), super::WindowError> {
        self.window.run(self.event_loop, animation, commands)?;
        Ok(())
    }
}

pub struct _WaylandWindow<T: Renderer> {
    layer: LayerSurface,
    renderer: Box<T>,
    compositor: CompositorState,
    layer_shell: LayerShell,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    pointer: Option<Pointer>,
    idle_notifier: Option<ExtIdleNotifierV1>,
    idle_notification: Option<ExtIdleNotificationV1>,
    output: Option<wl_output::WlOutput>,
    events: Sender<WindowEvent>,
    gesture: Gesture,
    placement: Placement,
    dock: Dock,
    exit: bool,
    error: Option<WaylandError>,
    width: u32,
    height: u32,
    presentation: Presentation,
    egl_window: WlEglSurface,
    qh: QueueHandle<Self>,
    conn: Connection,
    walk: Option<Walk>,
    heading_left: bool,
    loop_handle: LoopHandle<'static, Self>,
    theme: Arc<Theme>,
    requested: Animations,
    animation: Option<Animations>,
    hitbox: Option<(Rect, u32, u32)>,
    frame_delays: Vec<Duration>,
    loops: bool,
    frame: usize,
    frame_timer: Option<RegistrationToken>,
}

struct Pointer {
    device: wl_pointer::WlPointer,
    cursor: Option<WpCursorShapeDeviceV1>,
    enter_serial: Option<u32>,
}

type Position = (i32, i32);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Released,
    Pressed { at: (f64, f64) },
    Dragging { grab: (f64, f64) },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Anchored,
    Placed {
        applied: Position,
        in_flight: Option<Position>,
        target: Position,
    },
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

struct MoveAck;

struct IdleNotifierData;

struct IdleWatch {
    timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presentation {
    Unmapped,
    Idle,
    AwaitingFrame,
    AwaitingFrameDirty,
}

impl<T: Renderer + 'static> _WaylandWindow<T> {
    pub fn new(
        theme: Arc<Theme>,
        events: Sender<WindowEvent>,
    ) -> Result<(Self, EventLoop<'static, Self>), WaylandError> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init(&conn)?;
        let qh = event_queue.handle();

        let compositor = CompositorState::bind(&globals, &qh).context(BindSnafu {
            thing: "wl_compositor",
        })?;
        let layer_shell =
            LayerShell::bind(&globals, &qh).context(BindSnafu { thing: "wlr-shell" })?;
        let (width, height) = (SIZE, SIZE);
        let (layer, egl_window, window_handle) =
            create_layer(&compositor, &layer_shell, &qh, None, width, height)?;
        let display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr().cast()).ok_or(WaylandError::Display)?,
        ));
        let renderer = T::setup(display_handle, window_handle, width, height)?;

        let event_loop: EventLoop<Self> = EventLoop::try_new().context(CalloopSnafu {
            thing: "creating event loop",
        })?;
        WaylandSource::new(conn.clone(), event_queue)
            .insert(event_loop.handle())
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding wayland source",
            })?;
        Ok((
            Self {
                layer,
                renderer,
                compositor,
                layer_shell,
                registry_state: RegistryState::new(&globals),
                output_state: OutputState::new(&globals, &qh),
                seat_state: SeatState::new(&globals, &qh),
                cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
                pointer: None,
                idle_notifier: globals.bind(&qh, 1..=2, IdleNotifierData).ok(),
                idle_notification: None,
                output: None,
                events,
                gesture: Gesture::Released,
                placement: Placement::Anchored,
                dock: Dock::Free,
                exit: false,
                error: None,
                width,
                height,
                presentation: Presentation::Unmapped,
                egl_window,
                qh,
                conn,
                walk: None,
                heading_left: false,
                loop_handle: event_loop.handle(),
                theme,
                requested: Animations::default(),
                animation: None,
                hitbox: None,
                frame_delays: Vec::new(),
                loops: true,
                frame: 0,
                frame_timer: None,
            },
            event_loop,
        ))
    }
    pub fn run(
        &mut self,
        mut event_loop: EventLoop<'static, Self>,
        animation: Animations,
        commands: Channel<WindowCommand>,
    ) -> Result<(), WaylandError> {
        self.requested = animation;
        self.show()?;
        event_loop
            .handle()
            .insert_source(commands, |event, (), app| {
                let result = match event {
                    channel::Event::Msg(WindowCommand::Show(animation)) => {
                        app.requested = animation;
                        app.show()
                    }
                    channel::Event::Msg(WindowCommand::Roam(true)) => app.start_walk(),
                    channel::Event::Msg(WindowCommand::Roam(false)) => {
                        app.stop_walk(false);
                        Ok(())
                    }
                    channel::Event::Msg(WindowCommand::MoveToOutput(name)) => {
                        app.move_to_output(&name)
                    }
                    channel::Event::Closed => {
                        app.exit = true;
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    app.fail(e);
                }
            })
            .map_err(|e| e.error)
            .context(CalloopSnafu {
                thing: "adding command channel",
            })?;
        let loop_signal = event_loop.get_signal();

        event_loop
            .run(None, self, move |app| {
                if app.exit {
                    loop_signal.stop();
                }
            })
            .context(CalloopSnafu {
                thing: "event loop failed",
            })?;
        self.error.take().map_or(Ok(()), Err)
    }
    fn show(&mut self) -> Result<(), WaylandError> {
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
        self.frame_delays = loaded.frame_delays;
        self.loops = loaded.loops;
        self.frame = 0;
        if let Some(token) = self.frame_timer.take() {
            self.loop_handle.remove(token);
        }
        if let Some(delay) = self.frame_delays.first() {
            let token = self
                .loop_handle
                .insert_source(Timer::from_duration(*delay), |deadline, (), app| {
                    app.advance_frame(deadline)
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
    fn update_input_region(&self) -> Result<(), WaylandError> {
        let Some((hitbox, width, height)) = self.hitbox else {
            return Ok(());
        };
        let region = Region::new(&self.compositor)?;
        let (x, y, w, h) = input_rect(hitbox, (width, height), (self.width, self.height));
        let x = if self.mirrored() {
            self.width as i32 - x - w
        } else {
            x
        };
        region.add(x, y, w, h);
        self.layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
        Ok(())
    }
    fn set_cursor(&self, shape: Shape) {
        if let Some(Pointer {
            cursor: Some(cursor),
            enter_serial: Some(serial),
            ..
        }) = &self.pointer
        {
            cursor.set_shape(*serial, shape);
        }
    }
    fn pointer_event(
        &mut self,
        conn: &Connection,
        event: &PointerEvent,
    ) -> Result<(), WaylandError> {
        match event.kind {
            PointerEventKind::Enter { serial } => {
                if let Some(pointer) = &mut self.pointer {
                    pointer.enter_serial = Some(serial);
                }
                self.set_cursor(Shape::Default);
                self.peek(conn, true);
            }
            PointerEventKind::Leave { .. } => {
                self.end_gesture(conn)?;
                self.peek(conn, false);
            }
            PointerEventKind::Motion { .. } => self.pointer_moved(conn, event.position)?,
            PointerEventKind::Press {
                button: BUTTON_LEFT,
                ..
            } => {
                self.gesture = Gesture::Pressed { at: event.position };
            }
            PointerEventKind::Press {
                button: BUTTON_RIGHT,
                ..
            } => {
                if matches!(self.gesture, Gesture::Released) {
                    let _ = self.events.send(WindowEvent::OpenSettings);
                }
            }
            PointerEventKind::Release {
                button: BUTTON_LEFT,
                ..
            } => {
                if matches!(self.gesture, Gesture::Pressed { .. }) {
                    if matches!(self.dock, Dock::Docked { .. }) {
                        self.undock(conn)?;
                    } else {
                        let side = if event.position.0 < f64::from(self.width) / 2.0 {
                            Side::Left
                        } else {
                            Side::Right
                        };
                        let _ = self.events.send(WindowEvent::Click { side });
                    }
                }
                self.end_gesture(conn)?;
            }
            PointerEventKind::Press { .. }
            | PointerEventKind::Release { .. }
            | PointerEventKind::Axis { .. } => {}
        }
        Ok(())
    }
    fn pointer_moved(
        &mut self,
        conn: &Connection,
        position: (f64, f64),
    ) -> Result<(), WaylandError> {
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
                self.set_cursor(Shape::Grabbing);
                self.show()?;
                at
            }
        };
        let Placement::Placed { applied, .. } = self.placement else {
            return Ok(());
        };
        let target = self.clamp((
            applied.0 + (position.0 - grab.0).round() as i32,
            applied.1 + (position.1 - grab.1).round() as i32,
        ));
        self.move_to(conn, target);
        Ok(())
    }
    fn end_gesture(&mut self, conn: &Connection) -> Result<(), WaylandError> {
        let was_dragging = matches!(self.gesture, Gesture::Dragging { .. });
        self.gesture = Gesture::Released;
        if was_dragging {
            self.set_cursor(Shape::Default);
            self.show()?;
            self.try_dock(conn)?;
        }
        Ok(())
    }
    fn snap_zone(&self, side: Side) -> Option<i32> {
        let (output_width, _) = self.output_size()?;
        let width = self.width as i32;
        let overhang = width / 4;
        Some(match side {
            Side::Left => -overhang + SNAP_TOLERANCE,
            Side::Right => output_width - width + overhang - SNAP_TOLERANCE,
        })
    }
    fn in_snap_zone(&self, side: Side, x: i32) -> bool {
        self.snap_zone(side).is_some_and(|zone| match side {
            Side::Left => x <= zone,
            Side::Right => x >= zone,
        })
    }
    fn try_dock(&mut self, conn: &Connection) -> Result<(), WaylandError> {
        let (Some(mini), Placement::Placed { target, .. }, Some((output_width, _))) = (
            self.theme.behaviour().mini,
            self.placement,
            self.output_size(),
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
        let width = f64::from(self.width);
        let x = match side {
            Side::Left => -(width * mini.offset_ratio).round() as i32,
            Side::Right => output_width - (width * (1.0 - mini.offset_ratio)).round() as i32,
        };
        self.dock = Dock::Docked {
            side,
            restore: target,
            x,
            peeking: false,
        };
        self.move_to(conn, (x, target.1));
        let _ = self.events.send(WindowEvent::Docked);
        self.refresh_orientation()
    }
    fn peek(&mut self, conn: &Connection, inside: bool) {
        let (
            Dock::Docked {
                side,
                restore,
                x,
                peeking,
            },
            Placement::Placed { target, .. },
        ) = (self.dock, self.placement)
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
        self.move_to(conn, (x + offset, target.1));
        let _ = self.events.send(WindowEvent::Hover { inside });
    }
    fn undock(&mut self, conn: &Connection) -> Result<(), WaylandError> {
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
        self.move_to(conn, (x, restore.1));
        let _ = self.events.send(WindowEvent::Undocked);
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
    fn start_walk(&mut self) -> Result<(), WaylandError> {
        let walkable = self.walk.is_none()
            && self.dock == Dock::Free
            && self.gesture == Gesture::Released
            && self.place();
        let from = match self.placement {
            Placement::Placed { target, .. } if walkable => target,
            _ => {
                let _ = self.events.send(WindowEvent::RoamEnded);
                return Ok(());
            }
        };
        let Some(to) = self.roam_target(from) else {
            let _ = self.events.send(WindowEvent::RoamEnded);
            return Ok(());
        };
        let distance = f64::from(to.0 - from.0).hypot(f64::from(to.1 - from.1));
        let duration_ms = (distance / ROAM_PX_PER_MS).max(ROAM_MIN_DURATION_MS);
        let timer = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, (), app| app.walk_step())
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
        let (output_width, output_height) = self.output_size()?;
        let margin_x = (f64::from(output_width) * ROAM_MARGIN).round() as i32;
        let margin_y = (f64::from(output_height) * ROAM_MARGIN).round() as i32;
        let x_range = (margin_x, output_width - self.width as i32 - margin_x);
        let y_range = (margin_y, output_height - self.height as i32 - margin_y);
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
        let conn = self.conn.clone();
        self.move_to(&conn, position);
        if progress < 1.0 {
            return TimeoutAction::ToDuration(ROAM_STEP);
        }
        self.walk = None;
        let _ = self.events.send(WindowEvent::RoamEnded);
        TimeoutAction::Drop
    }
    fn stop_walk(&mut self, notify: bool) {
        if let Some(walk) = self.walk.take() {
            self.loop_handle.remove(walk.timer);
            if notify {
                let _ = self.events.send(WindowEvent::RoamEnded);
            }
        }
    }
    fn report_center(&self) {
        let Some(output) = self
            .output
            .as_ref()
            .and_then(|output| self.output_state.info(output))
        else {
            return;
        };
        let (Some((output_x, output_y)), Some((output_width, output_height))) =
            (output.logical_position, output.logical_size)
        else {
            return;
        };
        let (width, height) = (self.width as i32, self.height as i32);
        let (x, y) = match self.placement {
            Placement::Placed { applied, .. } => applied,
            Placement::Anchored => (output_width - width, (output_height - height) / 2),
        };
        let center = (
            f64::from(output_x + x) + f64::from(width) / 2.0,
            f64::from(output_y + y) + f64::from(height) / 2.0,
        );
        let _ = self.events.send(WindowEvent::Moved { center });
    }
    fn refresh_orientation(&mut self) -> Result<(), WaylandError> {
        self.update_input_region()?;
        self.invalidate();
        Ok(())
    }
    fn move_to(&mut self, conn: &Connection, target: Position) {
        let Placement::Placed {
            applied, in_flight, ..
        } = self.placement
        else {
            return;
        };
        self.placement = Placement::Placed {
            applied,
            in_flight,
            target,
        };
        if in_flight.is_none() && target != applied {
            self.commit_move(conn, target);
        }
    }
    fn move_to_output(&mut self, name: &str) -> Result<(), WaylandError> {
        let Some(output) = self.output_state.outputs().find(|output| {
            self.output_state
                .info(output)
                .and_then(|info| info.name)
                .is_some_and(|output_name| output_name == name)
        }) else {
            return Ok(());
        };
        if self.output.as_ref() == Some(&output) {
            return Ok(());
        }
        self.stop_walk(true);
        let (layer, egl_window, window_handle) = create_layer(
            &self.compositor,
            &self.layer_shell,
            &self.qh,
            Some(&output),
            self.width,
            self.height,
        )?;
        self.renderer
            .replace_window(window_handle, self.width, self.height)?;
        self.egl_window = egl_window;
        self.layer = layer;
        self.output = Some(output);
        self.presentation = Presentation::Unmapped;
        self.placement = Placement::Anchored;
        self.gesture = Gesture::Released;
        if matches!(self.dock, Dock::Docked { .. }) {
            self.dock = Dock::Free;
            let _ = self.events.send(WindowEvent::Undocked);
        }
        self.update_input_region()
    }
    fn watch_idle(&mut self, seat: &wl_seat::WlSeat) {
        let Some(notifier) = &self.idle_notifier else {
            return;
        };
        if self.idle_notification.is_some() {
            return;
        }
        let sleep = self.theme.behaviour().sleep;
        let timeout_ms = sleep.idle_after_ms.min(sleep.yawn_after_ms);
        let watch = IdleWatch {
            timeout: Duration::from_millis(u64::from(timeout_ms)),
        };
        self.idle_notification = Some(if notifier.version() >= INPUT_IDLE_VERSION {
            notifier.get_input_idle_notification(timeout_ms, seat, &self.qh, watch)
        } else {
            notifier.get_idle_notification(timeout_ms, seat, &self.qh, watch)
        });
    }
    fn output_size(&self) -> Option<(i32, i32)> {
        self.output
            .as_ref()
            .and_then(|output| self.output_state.info(output))
            .and_then(|info| info.logical_size)
    }
    fn place(&mut self) -> bool {
        if self.placement == Placement::Anchored {
            let Some((output_width, output_height)) = self.output_size() else {
                return false;
            };
            let origin = (
                output_width - self.width as i32,
                (output_height - self.height as i32) / 2,
            );
            self.layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            self.placement = Placement::Placed {
                applied: origin,
                in_flight: None,
                target: origin,
            };
        }
        true
    }
    fn clamp(&self, (x, y): Position) -> Position {
        let (output_width, output_height) = self.output_size().unwrap_or((i32::MAX, i32::MAX));
        let overhang = self.width as i32 / 4;
        let max_x = (output_width - self.width as i32 + overhang).max(-overhang);
        let max_y = (output_height - self.height as i32).max(0);
        (x.clamp(-overhang, max_x), y.clamp(0, max_y))
    }
    fn commit_move(&mut self, conn: &Connection, target: Position) {
        if let Placement::Placed {
            applied,
            target: latest,
            ..
        } = self.placement
        {
            self.layer.set_margin(target.1, 0, 0, target.0);
            self.layer.commit();
            conn.display().sync(&self.qh, MoveAck);
            self.placement = Placement::Placed {
                applied,
                in_flight: Some(target),
                target: latest,
            };
        }
    }
    fn move_acknowledged(&mut self, conn: &Connection) {
        if let Placement::Placed {
            in_flight: Some(applied),
            target,
            ..
        } = self.placement
        {
            self.placement = Placement::Placed {
                applied,
                in_flight: None,
                target,
            };
            self.report_center();
            if target != applied {
                self.commit_move(conn, target);
            }
        }
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
    fn fail(&mut self, error: WaylandError) {
        self.error = Some(error);
        self.exit = true;
    }
    fn draw(&mut self) -> Result<(), WaylandError> {
        self.presentation = Presentation::AwaitingFrame;
        let mirrored = self.mirrored();
        self.renderer.draw(
            self.width as i32,
            self.height as i32,
            self.frame as u32,
            mirrored,
        );
        self.layer
            .wl_surface()
            .frame(&self.qh, FrameCallbackData(self.layer.wl_surface().clone()));
        self.renderer.swapbuffers()?;
        self.layer.commit();
        Ok(())
    }
}

impl<T: Renderer + 'static> CompositorHandler for _WaylandWindow<T> {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if surface != self.layer.wl_surface() {
            return;
        }
        if self.presentation == Presentation::AwaitingFrameDirty {
            self.redraw();
        } else {
            self.presentation = Presentation::Idle;
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if self.output.is_none() {
            self.output = Some(output.clone());
        }
        self.report_center();
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}
impl<T: Renderer> OutputHandler for _WaylandWindow<T> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<T: Renderer + 'static> LayerShellHandler for _WaylandWindow<T> {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (mut new_width, mut new_height) = configure.new_size;
        if new_width == 0 {
            new_width = self.width;
        }
        if new_height == 0 {
            new_height = self.height;
        }

        if new_width != self.width || new_height != self.height {
            self.width = new_width;
            self.height = new_height;
            self.egl_window
                .resize(self.width as i32, self.height as i32, 0, 0);
            if let Err(e) = self.renderer.resize(self.width, self.height) {
                self.fail(WaylandError::Renderer { source: e });
                return;
            }
            if let Err(e) = self.update_input_region() {
                self.fail(e);
                return;
            }
            self.invalidate();
        }

        if self.presentation == Presentation::Unmapped {
            self.redraw();
        }
    }
}

delegate_registry!(@<T: Renderer+'static>_WaylandWindow<T>);

impl<T: Renderer + 'static> ProvidesRegistryState for _WaylandWindow<T> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl<T: Renderer + 'static> SeatHandler for _WaylandWindow<T> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.watch_idle(&seat);
        if capability != Capability::Pointer || self.pointer.is_some() {
            return;
        }
        match self.seat_state.get_pointer(qh, &seat) {
            Ok(pointer) => {
                let cursor = self
                    .cursor_shapes
                    .as_ref()
                    .map(|shapes| shapes.get_shape_device(&pointer, qh));
                self.pointer = Some(Pointer {
                    device: pointer,
                    cursor,
                    enter_serial: None,
                });
            }
            Err(e) => self.fail(e.into()),
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Pointer {
            return;
        }
        if let Some(Pointer { device, cursor, .. }) = self.pointer.take() {
            if let Some(cursor) = cursor {
                cursor.destroy();
            }
            device.release();
        }
        self.gesture = Gesture::Released;
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl<T: Renderer + 'static> PointerHandler for _WaylandWindow<T> {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            if let Err(e) = self.pointer_event(conn, event) {
                self.fail(e);
                return;
            }
        }
    }
}

impl<T: Renderer + 'static> Dispatch2<wl_callback::WlCallback, _WaylandWindow<T>> for MoveAck {
    fn event(
        &self,
        state: &mut _WaylandWindow<T>,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        conn: &Connection,
        _: &QueueHandle<_WaylandWindow<T>>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.move_acknowledged(conn);
        }
    }
}

impl<T: Renderer + 'static> Dispatch2<ExtIdleNotifierV1, _WaylandWindow<T>> for IdleNotifierData {
    fn event(
        &self,
        _: &mut _WaylandWindow<T>,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<_WaylandWindow<T>>,
    ) {
    }
}

impl<T: Renderer + 'static> Dispatch2<ExtIdleNotificationV1, _WaylandWindow<T>> for IdleWatch {
    fn event(
        &self,
        state: &mut _WaylandWindow<T>,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &Connection,
        _: &QueueHandle<_WaylandWindow<T>>,
    ) {
        let event = match event {
            ext_idle_notification_v1::Event::Idled => {
                let now = Instant::now();
                WindowEvent::UserIdle {
                    since: now.checked_sub(self.timeout).unwrap_or(now),
                }
            }
            ext_idle_notification_v1::Event::Resumed => WindowEvent::UserActive,
            _ => return,
        };
        let _ = state.events.send(event);
    }
}

fn create_layer<T: Renderer + 'static>(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<_WaylandWindow<T>>,
    output: Option<&wl_output::WlOutput>,
    width: u32,
    height: u32,
) -> Result<(LayerSurface, WlEglSurface, RawWindowHandle), WaylandError> {
    let surface = compositor.create_surface(qh);
    let layer = layer_shell.create_layer_surface(qh, surface, Layer::Top, Some(NAMESPACE), output);
    layer.set_anchor(Anchor::RIGHT);
    layer.set_size(width, height);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_margin(0, 0, 0, 0);
    layer.commit();
    let egl_window = WlEglSurface::new(layer.wl_surface().id(), width as i32, height as i32)
        .context(WaylandSnafu {
            thing: "egl_window",
        })?;
    let window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(layer.wl_surface().id().as_ptr().cast()).ok_or(WaylandError::Window)?,
    ));
    Ok((layer, egl_window, window_handle))
}

fn input_rect(hitbox: Rect, content: (u32, u32), surface: (u32, u32)) -> (i32, i32, i32, i32) {
    let (content_width, content_height) = (f64::from(content.0), f64::from(content.1));
    let (surface_width, surface_height) = (f64::from(surface.0), f64::from(surface.1));
    let scale = (surface_width / content_width).min(surface_height / content_height);
    let left = (content_width.mul_add(-scale, surface_width)) / 2.0;
    let top = (content_height.mul_add(-scale, surface_height)) / 2.0;
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

smithay_client_toolkit::delegate_dispatch2!(@<T:Renderer> _WaylandWindow<T>);
