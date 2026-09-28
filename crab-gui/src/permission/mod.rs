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

mod detail;
mod style;

use std::{
    io::{self, Read, Write},
    path::Path,
};

use crab_common::{
    agent::{AgentEvent, EventKind, MAX_MESSAGE_BYTES, PermissionDecision},
    gui::PERMISSION_APP_ID,
};
use iced::{
    Element, Fill, Font, Length, Size, Task,
    widget::{
        button, column, container, row, scrollable,
        scrollable::{Direction, Scrollbar},
        sensor, space, text,
    },
    window,
};
use snafu::{ResultExt, Snafu};

use crate::{
    desktop::{self, Desktop},
    theme::{self, BOLD, MEDIUM, SEMIBOLD},
};
use detail::Detail;
use style::Colors;

const TITLE: &str = "Permission Request";
const ALLOW_LABEL: &str = "Allow";
const DENY_LABEL: &str = "Deny";
const TERMINAL_LABEL: &str = "Go to Terminal";
const EXPAND_LABEL: &str = "View details";
const COLLAPSE_LABEL: &str = "Collapse";
const TRUNCATED_LABEL: &str = "Content is too large and has been truncated.";
const DENY_MESSAGE: &str = "Denied from crab-on-desk";
const SESSION_SEPARATOR: &str = " \u{b7} ";
const SESSION_ID_CHARS: usize = 3;

const COMPACT_WIDTH: f32 = 328.0;
const EXPANDED_WIDTH: f32 = 488.0;
const EXPANDED_MAX_HEIGHT: f32 = 608.0;
const INITIAL_HEIGHT: f32 = 220.0;
const CARD_PADDING: [u16; 2] = [16, 20];
const CARD_GAP: u32 = 8;
const HEADER_GAP: u32 = 4;
const PILL_PADDING: [u16; 2] = [3, 8];
const PREVIEW_PADDING: [u16; 2] = [10, 12];
const DETAIL_PADDING: u16 = 12;
const BUTTON_PADDING: [u16; 2] = [7, 0];
const SECONDARY_PADDING: [u16; 2] = [7, 12];
const COLLAPSE_PADDING: [u16; 2] = [5, 7];
const SCROLLBAR_WIDTH: f32 = 6.0;

const TITLE_SIZE: u32 = 13;
const PILL_SIZE: u32 = 11;
const TAG_SIZE: u32 = 11;
const CODE_SIZE: u32 = 12;
const BUTTON_SIZE: u32 = 13;
const SECONDARY_SIZE: u32 = 12;
const COLLAPSE_SIZE: u32 = 11;
const CODE_LINE_HEIGHT: f32 = 1.5;
const CODE_LINE: f32 = 18.0;
const PREVIEW_CLAMP: f32 = 54.0;
const PREVIEW_MAX_HEIGHT: f32 = 76.0;
const OVERFLOW_TOLERANCE: f32 = 1.0;
const MIN_DETAIL_LINES: f32 = 5.0;

#[derive(Debug, Snafu)]
pub enum PermissionError {
    #[snafu(display("Unable to read the permission request"))]
    Stdin { source: io::Error },
    #[snafu(display("Invalid permission request"))]
    Request { source: serde_json::Error },
    #[snafu(display("The request is not a permission request"))]
    NotPermission,
    #[snafu(display("Unable to show the permission prompt"))]
    Window { source: iced::Error },
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Allow,
    Deny,
    GoToTerminal,
    Expand,
    Collapse,
    CardSized(Size),
    PreviewSized(Size),
    DetailSized(Size),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Compact,
    Expanded,
}

#[derive(Debug, Clone)]
struct Request {
    tool: String,
    session: String,
    detail: Detail,
    terminal: Option<u32>,
}

struct Prompt {
    request: Request,
    colors: &'static Colors,
    desktop: Option<Box<dyn Desktop>>,
    error: Option<String>,
    layout: Layout,
    preview_overflows: bool,
    card_height: f32,
    detail_height: f32,
    viewport: f32,
    window: Size,
}

pub fn run() -> Result<(), PermissionError> {
    let request = read_request()?;
    let appearance = theme::system();
    let colors = style::colors(appearance);
    iced::application(
        move || Prompt {
            request: request.clone(),
            colors,
            desktop: desktop::detect(),
            error: None,
            layout: Layout::Compact,
            preview_overflows: false,
            card_height: INITIAL_HEIGHT,
            detail_height: 0.0,
            viewport: MIN_DETAIL_LINES * CODE_LINE,
            window: Size::new(COMPACT_WIDTH, INITIAL_HEIGHT),
        },
        Prompt::update,
        Prompt::view,
    )
    .title(TITLE)
    .theme(style::theme(appearance))
    .window(window_settings())
    .run()
    .context(WindowSnafu)
}

fn read_request() -> Result<Request, PermissionError> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_MESSAGE_BYTES)
        .read_to_end(&mut input)
        .context(StdinSnafu)?;
    let event: AgentEvent = serde_json::from_slice(&input).context(RequestSnafu)?;
    let EventKind::PermissionRequest { tool_input } = &event.kind else {
        return NotPermissionSnafu.fail();
    };
    let tool = event.tool_name.clone().unwrap_or_default();
    Ok(Request {
        detail: detail::describe(&tool, tool_input),
        session: session_tag(&event),
        tool,
        terminal: event.source_pid,
    })
}

fn session_tag(event: &AgentEvent) -> String {
    let folder = event
        .cwd
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let skip = event
        .session_id
        .chars()
        .count()
        .saturating_sub(SESSION_ID_CHARS);
    let short_id = format!(
        "#{}",
        event.session_id.chars().skip(skip).collect::<String>()
    );
    folder
        .into_iter()
        .chain([short_id])
        .collect::<Vec<_>>()
        .join(SESSION_SEPARATOR)
}

fn window_settings() -> window::Settings {
    let size = Size::new(COMPACT_WIDTH, INITIAL_HEIGHT);
    window::Settings {
        size,
        min_size: Some(size),
        max_size: Some(size),
        platform_specific: platform_settings(),
        ..window::Settings::default()
    }
}

#[cfg(target_os = "linux")]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        application_id: PERMISSION_APP_ID.to_owned(),
        ..window::settings::PlatformSpecific::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

fn decide(decision: &PermissionDecision) -> Task<Message> {
    let mut stdout = io::stdout().lock();
    let written = serde_json::to_writer(&mut stdout, decision)
        .map_err(io::Error::from)
        .and_then(|()| writeln!(stdout))
        .and_then(|()| stdout.flush());
    if let Err(error) = written {
        eprintln!("Unable to write the permission decision: {error}");
    }
    iced::exit()
}

fn hidden_scroll() -> Direction {
    Direction::Vertical(Scrollbar::hidden())
}

impl Prompt {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Allow => decide(&PermissionDecision::Allow),
            Message::Deny => decide(&PermissionDecision::Deny {
                message: DENY_MESSAGE.to_owned(),
            }),
            Message::GoToTerminal => self.go_to_terminal(),
            Message::Expand => {
                self.layout = Layout::Expanded;
                self.fit()
            }
            Message::Collapse => {
                self.layout = Layout::Compact;
                self.fit()
            }
            Message::CardSized(size) => {
                self.card_height = size.height;
                self.fit()
            }
            Message::PreviewSized(size) => {
                self.preview_overflows = size.height > PREVIEW_CLAMP + OVERFLOW_TOLERANCE;
                Task::none()
            }
            Message::DetailSized(size) => {
                self.detail_height = size.height;
                self.fit()
            }
        }
    }

    fn go_to_terminal(&mut self) -> Task<Message> {
        let (Some(desktop), Some(pid)) = (&self.desktop, self.request.terminal) else {
            return Task::none();
        };
        match desktop.focus(pid) {
            Ok(()) => iced::exit(),
            Err(error) => {
                self.error = Some(snafu::Report::from_error(error).to_string());
                Task::none()
            }
        }
    }

    fn fit(&mut self) -> Task<Message> {
        let size = match self.layout {
            Layout::Compact => Size::new(COMPACT_WIDTH, self.card_height),
            Layout::Expanded => {
                let chrome = self.card_height - self.viewport;
                let readable = MIN_DETAIL_LINES * CODE_LINE;
                self.viewport = self
                    .detail_height
                    .min((EXPANDED_MAX_HEIGHT - chrome).max(readable));
                Size::new(EXPANDED_WIDTH, chrome + self.viewport)
            }
        };
        let size = Size::new(size.width, size.height.ceil());
        if size == self.window {
            return Task::none();
        }
        self.window = size;
        window::latest().and_then(move |id| {
            window::set_max_size(id, None)
                .chain(window::set_min_size(id, None))
                .chain(window::resize(id, size))
                .chain(window::set_min_size(id, Some(size)))
                .chain(window::set_max_size(id, Some(size)))
        })
    }

    fn needs_expansion(&self) -> bool {
        let detail = &self.request.detail;
        detail.full != detail.preview || detail.truncated || self.preview_overflows
    }

    fn view(&self) -> Element<'_, Message> {
        let card = column![self.header()]
            .push((!self.request.session.is_empty()).then(|| self.session()))
            .push(match self.layout {
                Layout::Compact => self.preview(),
                Layout::Expanded => self.detail(),
            })
            .push(
                (self.layout == Layout::Compact && self.needs_expansion()).then(|| {
                    self.secondary_button(
                        EXPAND_LABEL,
                        SEMIBOLD,
                        self.colors.secondary_hover_text,
                        Message::Expand,
                    )
                }),
            )
            .push(self.actions())
            .push(
                (self.desktop.is_some() && self.request.terminal.is_some()).then(|| {
                    self.secondary_button(
                        TERMINAL_LABEL,
                        MEDIUM,
                        self.colors.secondary_text,
                        Message::GoToTerminal,
                    )
                }),
            )
            .push(
                self.error
                    .as_deref()
                    .map(|error| text(error).size(TAG_SIZE).color(self.colors.warning)),
            )
            .spacing(CARD_GAP)
            .padding(CARD_PADDING)
            .width(Fill);
        scrollable(
            sensor(card)
                .on_show(Message::CardSized)
                .on_resize(Message::CardSized),
        )
        .direction(hidden_scroll())
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn header(&self) -> Element<'_, Message> {
        let pill = container(
            text(self.request.tool.to_uppercase())
                .size(PILL_SIZE)
                .font(BOLD),
        )
        .padding(PILL_PADDING)
        .style(|_| style::pill(style::tool_color(&self.request.tool)));
        let title = column![
            text(TITLE)
                .size(TITLE_SIZE)
                .font(SEMIBOLD)
                .color(self.colors.header),
            pill,
        ]
        .spacing(HEADER_GAP);
        let colors = self.colors;
        let collapse = (self.layout == Layout::Expanded).then(|| {
            button(text(COLLAPSE_LABEL).size(COLLAPSE_SIZE).font(SEMIBOLD))
                .padding(COLLAPSE_PADDING)
                .style(move |_, status| style::plain(colors, status))
                .on_press(Message::Collapse)
        });
        row![title, space().width(Fill)]
            .push(collapse)
            .spacing(HEADER_GAP)
            .into()
    }

    fn session(&self) -> Element<'_, Message> {
        text(&self.request.session)
            .size(TAG_SIZE)
            .font(Font::MONOSPACE)
            .color(style::faded(self.colors.code_text))
            .wrapping(text::Wrapping::None)
            .into()
    }

    fn code(content: &str) -> text::Text<'_> {
        text(content.to_owned())
            .size(CODE_SIZE)
            .font(Font::MONOSPACE)
            .line_height(CODE_LINE_HEIGHT)
            .wrapping(text::Wrapping::WordOrGlyph)
            .width(Fill)
    }

    fn preview(&self) -> Element<'_, Message> {
        let colors = self.colors;
        container(
            scrollable(
                sensor(Self::code(&self.request.detail.preview))
                    .on_show(Message::PreviewSized)
                    .on_resize(Message::PreviewSized),
            )
            .direction(hidden_scroll())
            .height(Length::Shrink),
        )
        .padding(PREVIEW_PADDING)
        .max_height(PREVIEW_MAX_HEIGHT)
        .clip(true)
        .width(Fill)
        .style(move |_| style::code_block(colors))
        .into()
    }

    fn detail(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let block = container(Self::code(&self.request.detail.full))
            .padding(DETAIL_PADDING)
            .width(Fill)
            .style(move |_| style::code_block(colors));
        let scroll = scrollable(
            sensor(block)
                .on_show(Message::DetailSized)
                .on_resize(Message::DetailSized),
        )
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH),
        ))
        .height(self.viewport);
        column![]
            .push(self.request.detail.truncated.then(|| {
                text(TRUNCATED_LABEL)
                    .size(TAG_SIZE)
                    .color(self.colors.warning)
            }))
            .push(scroll)
            .spacing(CARD_GAP)
            .into()
    }

    fn actions(&self) -> Element<'_, Message> {
        let colors = self.colors;
        row![
            button(
                text(ALLOW_LABEL)
                    .size(BUTTON_SIZE)
                    .font(SEMIBOLD)
                    .center()
                    .width(Fill)
            )
            .width(Fill)
            .padding(BUTTON_PADDING)
            .style(style::allow)
            .on_press(Message::Allow),
            button(
                text(DENY_LABEL)
                    .size(BUTTON_SIZE)
                    .font(SEMIBOLD)
                    .center()
                    .width(Fill)
            )
            .width(Fill)
            .padding(BUTTON_PADDING)
            .style(move |_, status| style::deny(colors, status))
            .on_press(Message::Deny),
        ]
        .spacing(CARD_GAP)
        .into()
    }

    fn secondary_button(
        &self,
        label: &'static str,
        font: Font,
        text_color: iced::Color,
        message: Message,
    ) -> Element<'_, Message> {
        let colors = self.colors;
        button(
            text(label)
                .size(SECONDARY_SIZE)
                .font(font)
                .center()
                .width(Fill),
        )
        .width(Fill)
        .padding(SECONDARY_PADDING)
        .style(move |_, status| style::secondary(colors, text_color, status))
        .on_press(message)
        .into()
    }
}
