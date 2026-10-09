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
    collections::VecDeque,
    num::NonZeroU32,
    os::fd::{AsFd, BorrowedFd},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory,
    channel::Sender, generic::Generic,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XcbDisplayHandle, XcbWindowHandle};
use snafu::{OptionExt, ResultExt};
use x11rb::{
    COPY_DEPTH_FROM_PARENT, NONE, atom_manager,
    connection::Connection,
    cursor::Handle as CursorThemes,
    properties::{WmHints, WmSizeHints, WmSizeHintsSpecification},
    protocol::{
        Event,
        randr::{self, ConnectionExt as _},
        shape::{self, ConnectionExt as _},
        sync::{self, ConnectionExt as _},
        xproto::{
            AtomEnum, ChangeWindowAttributesAux, ColormapAlloc, ConfigureWindowAux,
            ConnectionExt as _, CreateWindowAux, Cursor, EventMask, KeyButMask, NotifyMode,
            PropMode, Rectangle, Screen, VisualClass, Visualid, Window as XWindow, WindowClass,
        },
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};

use super::{
    CalloopSnafu, NAMESPACE, SIZE, Window, WindowError, WindowEvent,
    widget::{
        Button, CursorShape, Output, OutputChange, Pointer, Position, Running, Surface, Widget,
    },
};
use crate::{
    backend::x11::{self as backend, InvalidWindowSnafu, NoScreenSnafu, X11Error},
    placement::Area,
    renderer::Renderer,
    theme::Theme,
};

const ARGB_DEPTH: u8 = 32;
const BUTTON_LEFT: u8 = 1;
const BUTTON_RIGHT: u8 = 3;
const IDLE_COUNTER: &[u8] = b"IDLETIME";
const SYNC_VERSION: (u8, u8) = (3, 1);
const ALL_DESKTOPS: u32 = u32::MAX;
const MOTIF_DECORATIONS: u32 = 2;
const DEFAULT_CURSOR: &str = "default";
const GRABBING_CURSOR: &str = "grabbing";

atom_manager! {
    Atoms: AtomsCookie {
        UTF8_STRING,
        _NET_WM_NAME,
        _NET_WM_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _MOTIF_WM_HINTS,
    }
}

type State<R> = Widget<X11, R>;

pub struct X11 {
    conn: Rc<XCBConnection>,
    root: XWindow,
    window: XWindow,
    monitors: Vec<Output>,
    output: usize,
    position: Position,
    cursors: Cursors,
    composited: bool,
    idle: Option<IdleWatch>,
}

struct Cursors {
    default: Cursor,
    grabbing: Cursor,
}

struct IdleWatch {
    alarm: sync::Alarm,
    timeout_ms: i64,
    idle: bool,
}

pub fn open<R: Renderer + 'static>(
    theme: Arc<Theme>,
    events: Sender<WindowEvent>,
) -> Result<Box<dyn Window>, WindowError> {
    let (conn, number) = backend::connect()?;
    let conn = Rc::new(conn);
    let screen = backend::screen(&*conn, number)?.clone();
    let resources = backend::resources(&*conn)?;
    let extent = (f64::from(SIZE) * backend::scale(&resources)).round() as u32;
    let size = (extent, extent);
    let monitors = monitors(&conn, screen.root)?;
    let area = monitors
        .first()
        .context(NoScreenSnafu { screen: number })?
        .area;
    let position = (
        area.width - extent as i32,
        (area.height - extent as i32) / 2,
    );
    let (depth, visual) =
        argb_visual(&screen).unwrap_or((COPY_DEPTH_FROM_PARENT, screen.root_visual));
    let window = create_window(&conn, &screen, depth, visual, area, position, size)?;
    let themes = CursorThemes::new(&*conn, number, &resources)
        .map_err(X11Error::from)?
        .reply()
        .map_err(X11Error::from)?;
    let cursors = Cursors {
        default: themes
            .load_cursor(&*conn, DEFAULT_CURSOR)
            .map_err(X11Error::from)?,
        grabbing: themes
            .load_cursor(&*conn, GRABBING_CURSOR)
            .map_err(X11Error::from)?,
    };
    let composited = composited(&conn, number)?;
    if !composited {
        eprintln!("No compositing manager is running: the crab is drawn without transparency");
    }
    let _ = conn.randr_select_input(
        screen.root,
        randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::CRTC_CHANGE,
    );
    conn.map_window(window).map_err(X11Error::from)?;
    conn.flush().map_err(X11Error::from)?;
    let display = RawDisplayHandle::Xcb(XcbDisplayHandle::new(
        NonNull::new(conn.get_raw_xcb_connection()),
        number as i32,
    ));
    let mut window_handle =
        XcbWindowHandle::new(NonZeroU32::new(window).context(InvalidWindowSnafu)?);
    window_handle.visual_id = NonZeroU32::new(visual);
    let renderer = R::setup(display, RawWindowHandle::Xcb(window_handle), size.0, size.1)?;
    let event_loop: EventLoop<State<R>> = EventLoop::try_new().context(CalloopSnafu {
        thing: "creating event loop",
    })?;
    event_loop
        .handle()
        .insert_source(Source::new(Rc::clone(&conn)), |event, (), widget| {
            widget.x11_event(event);
        })
        .map_err(|e| e.error)
        .context(CalloopSnafu {
            thing: "adding X11 source",
        })?;
    let surface = X11 {
        conn,
        root: screen.root,
        window,
        monitors,
        output: 0,
        position,
        cursors,
        composited,
        idle: None,
    };
    let mut widget = Widget::new(surface, renderer, size, theme, events, event_loop.handle());
    let timeout = widget.idle_timeout();
    widget.surface.idle = watch_idle(&widget.surface.conn, timeout).unwrap_or_else(|error| {
        eprintln!(
            "User idle detection disabled: {}",
            snafu::Report::from_error(error)
        );
        None
    });
    Ok(Box::new(Running { widget, event_loop }))
}

fn argb_visual(screen: &Screen) -> Option<(u8, Visualid)> {
    screen
        .allowed_depths
        .iter()
        .filter(|depth| depth.depth == ARGB_DEPTH)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.class == VisualClass::TRUE_COLOR)
        .map(|visual| (ARGB_DEPTH, visual.visual_id))
}

fn create_window(
    conn: &XCBConnection,
    screen: &Screen,
    depth: u8,
    visual: Visualid,
    area: Area,
    (x, y): Position,
    (width, height): (u32, u32),
) -> Result<XWindow, X11Error> {
    let atoms = Atoms::new(conn)?.reply()?;
    let colormap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual)?;
    let window = conn.generate_id()?;
    let (x, y) = (area.x + x, area.y + y);
    conn.create_window(
        depth,
        window,
        screen.root,
        x as i16,
        y as i16,
        width as u16,
        height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .colormap(colormap)
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::BUTTON1_MOTION
                    | EventMask::ENTER_WINDOW
                    | EventMask::LEAVE_WINDOW,
            ),
    )?;
    let name = NAMESPACE.as_bytes();
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        name,
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        atoms._NET_WM_NAME,
        atoms.UTF8_STRING,
        name,
    )?;
    let class = [name, b"\0", name, b"\0"].concat();
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        &class,
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms._NET_WM_WINDOW_TYPE,
        AtomEnum::ATOM,
        &[atoms._NET_WM_WINDOW_TYPE_UTILITY],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms._NET_WM_STATE,
        AtomEnum::ATOM,
        &[
            atoms._NET_WM_STATE_ABOVE,
            atoms._NET_WM_STATE_STICKY,
            atoms._NET_WM_STATE_SKIP_TASKBAR,
            atoms._NET_WM_STATE_SKIP_PAGER,
        ],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms._NET_WM_DESKTOP,
        AtomEnum::CARDINAL,
        &[ALL_DESKTOPS],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms._MOTIF_WM_HINTS,
        atoms._MOTIF_WM_HINTS,
        &[MOTIF_DECORATIONS, 0, 0, 0, 0],
    )?;
    let mut hints = WmHints::new();
    hints.input = Some(false);
    hints.set(conn, window)?;
    let (width, height) = (width as i32, height as i32);
    WmSizeHints {
        position: Some((WmSizeHintsSpecification::UserSpecified, x, y)),
        size: Some((WmSizeHintsSpecification::UserSpecified, width, height)),
        min_size: Some((width, height)),
        max_size: Some((width, height)),
        ..WmSizeHints::default()
    }
    .set_normal_hints(conn, window)?;
    Ok(window)
}

fn composited(conn: &XCBConnection, screen: usize) -> Result<bool, X11Error> {
    let selection = format!("_NET_WM_CM_S{screen}");
    let atom = conn.intern_atom(false, selection.as_bytes())?.reply()?.atom;
    Ok(conn.get_selection_owner(atom)?.reply()?.owner != NONE)
}

fn monitors(conn: &XCBConnection, root: XWindow) -> Result<Vec<Output>, X11Error> {
    if let Ok(monitors) = randr_monitors(conn, root)
        && !monitors.is_empty()
    {
        return Ok(monitors);
    }
    let geometry = conn.get_geometry(root)?.reply()?;
    Ok(vec![Output {
        name: None,
        area: Area {
            x: 0,
            y: 0,
            width: geometry.width.into(),
            height: geometry.height.into(),
        },
    }])
}

fn randr_monitors(conn: &XCBConnection, root: XWindow) -> Result<Vec<Output>, X11Error> {
    Ok(crab_common::x11::monitors(conn, root)?
        .into_iter()
        .map(|monitor| Output {
            name: Some(monitor.name),
            area: Area {
                x: monitor.x,
                y: monitor.y,
                width: monitor.width,
                height: monitor.height,
            },
        })
        .collect())
}

const fn int64(value: i64) -> sync::Int64 {
    sync::Int64 {
        hi: (value >> 32) as i32,
        lo: value as u32,
    }
}

fn from_int64(value: sync::Int64) -> i64 {
    (i64::from(value.hi) << 32) | i64::from(value.lo)
}

fn watch_idle(conn: &XCBConnection, timeout: Duration) -> Result<Option<IdleWatch>, X11Error> {
    conn.sync_initialize(SYNC_VERSION.0, SYNC_VERSION.1)?
        .reply()?;
    let Some(counter) = conn
        .sync_list_system_counters()?
        .reply()?
        .counters
        .into_iter()
        .find(|counter| counter.name == IDLE_COUNTER)
    else {
        return Ok(None);
    };
    let timeout_ms = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
    let alarm = conn.generate_id()?;
    conn.sync_create_alarm(
        alarm,
        &sync::CreateAlarmAux::new()
            .counter(counter.counter)
            .value_type(sync::VALUETYPE::ABSOLUTE)
            .value(int64(timeout_ms))
            .test_type(sync::TESTTYPE::POSITIVE_COMPARISON)
            .delta(int64(0))
            .events(1),
    )?;
    Ok(Some(IdleWatch {
        alarm,
        timeout_ms,
        idle: false,
    }))
}

impl X11 {
    fn area(&self) -> Option<Area> {
        self.monitors.get(self.output).map(|output| output.area)
    }

    fn local(&self, root_x: i16, root_y: i16) -> (f64, f64) {
        let area = self.area().unwrap_or(Area {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        (
            f64::from(i32::from(root_x) - area.x - self.position.0),
            f64::from(i32::from(root_y) - area.y - self.position.1),
        )
    }

    fn monitor(&self, name: &str) -> Option<usize> {
        self.monitors
            .iter()
            .position(|output| output.name.as_deref() == Some(name))
    }

    fn refresh_monitors(&mut self) -> Result<(), X11Error> {
        let current = self
            .monitors
            .get(self.output)
            .and_then(|output| output.name.clone());
        self.monitors = monitors(&self.conn, self.root)?;
        self.output = self
            .monitors
            .iter()
            .position(|output| output.name == current)
            .unwrap_or(0);
        Ok(())
    }

    fn idle_changed(&mut self, alarm: sync::Alarm, counter: sync::Int64) -> Option<WindowEvent> {
        let idle = self.idle.as_mut().filter(|idle| idle.alarm == alarm)?;
        let idle_ms = from_int64(counter);
        let now_idle = idle_ms >= idle.timeout_ms;
        if now_idle == idle.idle {
            return None;
        }
        idle.idle = now_idle;
        let (test, value) = if now_idle {
            (sync::TESTTYPE::NEGATIVE_COMPARISON, idle.timeout_ms - 1)
        } else {
            (sync::TESTTYPE::POSITIVE_COMPARISON, idle.timeout_ms)
        };
        let _ = self.conn.sync_change_alarm(
            alarm,
            &sync::ChangeAlarmAux::new()
                .test_type(test)
                .value(int64(value)),
        );
        Some(if now_idle {
            let now = Instant::now();
            let since = Duration::from_millis(u64::try_from(idle_ms).unwrap_or(0));
            WindowEvent::UserIdle {
                since: now.checked_sub(since).unwrap_or(now),
            }
        } else {
            WindowEvent::UserActive
        })
    }
}

impl Surface for X11 {
    fn output(&self) -> Option<Output> {
        self.monitors.get(self.output).cloned()
    }

    fn origin(&self) -> Option<Position> {
        Some(self.position)
    }

    fn target(&self) -> Option<Position> {
        Some(self.position)
    }

    fn place(&mut self, _home: Position) {}

    fn move_to(&mut self, target: Position) -> bool {
        self.position = target;
        if let Some(area) = self.area() {
            let _ = self.conn.configure_window(
                self.window,
                &ConfigureWindowAux::new()
                    .x(area.x + target.0)
                    .y(area.y + target.1),
            );
        }
        true
    }

    fn set_input_region(&mut self, region: Area) -> Result<(), WindowError> {
        let rectangle = Rectangle {
            x: region.x as i16,
            y: region.y as i16,
            width: region.width as u16,
            height: region.height as u16,
        };
        let kinds: &[shape::SK] = if self.composited {
            &[shape::SK::INPUT]
        } else {
            &[shape::SK::INPUT, shape::SK::BOUNDING]
        };
        for kind in kinds {
            self.conn
                .shape_rectangles(
                    shape::SO::SET,
                    *kind,
                    x11rb::protocol::xproto::ClipOrdering::UNSORTED,
                    self.window,
                    0,
                    0,
                    &[rectangle],
                )
                .map_err(X11Error::from)?;
        }
        Ok(())
    }

    fn set_cursor(&mut self, shape: CursorShape) {
        let cursor = match shape {
            CursorShape::Default => self.cursors.default,
            CursorShape::Grabbing => self.cursors.grabbing,
        };
        let _ = self.conn.change_window_attributes(
            self.window,
            &ChangeWindowAttributesAux::new().cursor(cursor),
        );
    }

    fn move_to_output<R: Renderer>(
        &mut self,
        name: &str,
        _renderer: &mut R,
    ) -> Result<OutputChange, WindowError> {
        if self.monitor(name).is_none() {
            self.refresh_monitors()?;
        }
        let Some(index) = self.monitor(name) else {
            return Ok(OutputChange::Unchanged);
        };
        if index == self.output {
            return Ok(OutputChange::Unchanged);
        }
        self.output = index;
        Ok(OutputChange::Moved)
    }

    fn present<R: Renderer>(&mut self, renderer: &R) -> Result<bool, WindowError> {
        renderer.swapbuffers()?;
        Ok(false)
    }
}

impl<R: Renderer + 'static> State<R> {
    fn x11_event(&mut self, event: Event) {
        let held = |state: KeyButMask| state.contains(KeyButMask::BUTTON1);
        let button = |detail| match detail {
            BUTTON_LEFT => Some(Button::Left),
            BUTTON_RIGHT => Some(Button::Right),
            _ => None,
        };
        let pointer = match event {
            Event::Expose(expose) if expose.count == 0 => {
                self.exposed();
                None
            }
            Event::ButtonPress(press) => button(press.detail).map(|button| {
                Pointer::Press(button, self.surface.local(press.root_x, press.root_y))
            }),
            Event::ButtonRelease(release) => button(release.detail).map(|button| {
                Pointer::Release(button, self.surface.local(release.root_x, release.root_y))
            }),
            Event::MotionNotify(motion) => Some(Pointer::Motion(
                self.surface.local(motion.root_x, motion.root_y),
            )),
            Event::EnterNotify(enter) if enter.mode == NotifyMode::NORMAL && !held(enter.state) => {
                Some(Pointer::Enter)
            }
            Event::LeaveNotify(leave) if leave.mode == NotifyMode::NORMAL && !held(leave.state) => {
                Some(Pointer::Leave)
            }
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => {
                match self.surface.refresh_monitors() {
                    Ok(()) => self.report_location(),
                    Err(error) => self.fail(error),
                }
                None
            }
            Event::SyncAlarmNotify(notify) => {
                if let Some(event) = self
                    .surface
                    .idle_changed(notify.alarm, notify.counter_value)
                {
                    self.send(event);
                }
                None
            }
            Event::Error(error) => {
                eprintln!("X11 error: {error:?}");
                None
            }
            _ => None,
        };
        if let Some(pointer) = pointer {
            self.pointer(pointer);
        }
    }
}

struct SharedFd(Rc<XCBConnection>);

impl AsFd for SharedFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

struct Source {
    conn: Rc<XCBConnection>,
    fd: Generic<SharedFd>,
    token: Option<Token>,
    pending: VecDeque<Event>,
}

impl Source {
    fn new(conn: Rc<XCBConnection>) -> Self {
        Self {
            fd: Generic::new(SharedFd(Rc::clone(&conn)), Interest::READ, Mode::Level),
            conn,
            token: None,
            pending: VecDeque::new(),
        }
    }

    fn next(&mut self) -> Result<Option<Event>, X11Error> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        Ok(self.conn.poll_for_event()?)
    }
}

impl EventSource for Source {
    type Event = Event;
    type Metadata = ();
    type Ret = ();
    type Error = X11Error;

    const NEEDS_EXTRA_LIFECYCLE_EVENTS: bool = true;

    fn process_events<F>(
        &mut self,
        _readiness: Readiness,
        _token: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        while let Some(event) = self.next()? {
            callback(event, &mut ());
        }
        Ok(PostAction::Continue)
    }

    fn register(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.token = Some(factory.token());
        self.fd.register(poll, factory)
    }

    fn reregister(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.token = Some(factory.token());
        self.fd.reregister(poll, factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.token = None;
        self.fd.unregister(poll)
    }

    fn before_sleep(&mut self) -> calloop::Result<Option<(Readiness, Token)>> {
        let other = |error: X11Error| calloop::Error::OtherError(Box::new(error));
        self.conn.flush().map_err(|e| other(e.into()))?;
        if let Some(event) = self.conn.poll_for_event().map_err(|e| other(e.into()))? {
            self.pending.push_back(event);
        }
        Ok(self
            .token
            .filter(|_| !self.pending.is_empty())
            .map(|token| (Readiness::EMPTY, token)))
    }
}
