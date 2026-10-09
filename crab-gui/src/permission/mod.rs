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
mod request;
mod style;

use std::io::{self, Read, Write};

use crab_common::{
    agent::{AgentEvent, MAX_MESSAGE_BYTES, PermissionDecision},
    gui::{PERMISSION_APP_ID, PERMISSION_MAX_HEIGHT, PromptSpot},
};
use iced::{
    Element, Fill, Font, Length, Point, Size, Subscription, Task,
    alignment::Horizontal,
    widget::{
        Column, button, column, container, operation, row, scrollable,
        scrollable::{Direction, Scrollbar},
        sensor, space, text, text_input,
    },
    window,
};
use snafu::{ResultExt, Snafu};

use crate::{
    desktop::{self, Desktop},
    keyboard,
    theme::{self, BOLD, MEDIUM, SEMIBOLD},
};
use request::{Answer, Kind, Question, Request};
use style::Colors;

const PERMISSION_TITLE: &str = "Permission Request";
const PLAN_TITLE: &str = "Plan Review";
const QUESTION_TITLE: &str = "Needs Input";
const ALLOW_LABEL: &str = "Allow";
const DENY_LABEL: &str = "Deny";
const APPROVE_LABEL: &str = "Approve";
const REJECT_LABEL: &str = "Reject";
const FEEDBACK_LABEL: &str = "Suggest changes";
const FEEDBACK_PLACEHOLDER: &str = "What should be changed?";
const SEND_LABEL: &str = "Send";
const BACK_LABEL: &str = "Back";
const NEXT_LABEL: &str = "Next";
const SUBMIT_LABEL: &str = "Submit Answer";
const OTHER_LABEL: &str = "Other";
const OTHER_PLACEHOLDER: &str = "Type your answer\u{2026}";
const SINGLE_HINT: &str = "Choose one option";
const MULTI_HINT: &str = "Multi-select, choose at least one";
const TERMINAL_LABEL: &str = "Go to Terminal";
const DENY_MESSAGE: &str = "Denied from crab-on-desk";
const TEXT_INPUT_ID: &str = "crab-permission-input";
const SELECTED_MARK: &str = "\u{25cf}";
const UNSELECTED_MARK: &str = "\u{25cb}";
const CHECKED_MARK: &str = "\u{25a0}";
const UNCHECKED_MARK: &str = "\u{25a1}";

const COMPACT_WIDTH: f32 = 328.0;
const PLAN_WIDTH: f32 = 488.0;
const INITIAL_HEIGHT: f32 = 220.0;
const CARD_PADDING: [u16; 2] = [16, 20];
const CARD_GAP: u32 = 8;
const HEADER_GAP: u32 = 4;
const OPTION_GAP: u32 = 6;
const PILL_PADDING: [u16; 2] = [3, 8];
const CODE_PADDING: [u16; 2] = [10, 12];
const QUESTION_PADDING: u16 = 12;
const BUTTON_PADDING: [u16; 2] = [7, 0];
const SECONDARY_PADDING: [u16; 2] = [7, 12];
const OPTION_PADDING: [u16; 2] = [8, 10];
const INPUT_PADDING: u16 = 8;
const SCROLLBAR_WIDTH: f32 = 6.0;
const TOOL_MAX_HEIGHT: f32 = 160.0;
const PLAN_MAX_HEIGHT: f32 = 320.0;

const TITLE_SIZE: u32 = 13;
const PILL_SIZE: u32 = 11;
const TAG_SIZE: u32 = 11;
const CODE_SIZE: u32 = 12;
const BUTTON_SIZE: u32 = 13;
const SECONDARY_SIZE: u32 = 12;
const QUESTION_SIZE: u32 = 13;
const CODE_LINE_HEIGHT: f32 = 1.5;

#[derive(Debug, Snafu)]
pub enum PermissionError {
    #[snafu(display("Unable to read the permission request"))]
    Stdin { source: io::Error },
    #[snafu(display("Invalid permission request"))]
    Request { source: serde_json::Error },
    #[snafu(display("Unable to show the permission prompt"))]
    Window { source: iced::Error },
}

#[derive(Debug, Clone)]
enum Message {
    Allow,
    Deny,
    Suggest(usize),
    GoToTerminal,
    OpenFeedback,
    CloseFeedback,
    FeedbackChanged(String),
    SendFeedback,
    Toggle(usize),
    ToggleOther,
    OtherChanged(String),
    Next,
    Back,
    CardSized(Size),
    WindowResized(Size),
    Close,
}

struct Prompt {
    request: Request,
    colors: &'static Colors,
    desktop: Option<Box<dyn Desktop>>,
    error: Option<String>,
    feedback: Option<String>,
    question: usize,
    answers: Vec<Answer>,
    card_height: f32,
    window: Size,
    spot: Option<PromptSpot>,
}

pub fn run(spot: Option<PromptSpot>) -> Result<(), PermissionError> {
    let Some(request) = read_request()? else {
        return Ok(());
    };
    let appearance = theme::system();
    let colors = style::colors(appearance);
    let initial = Size::new(width(&request.kind), INITIAL_HEIGHT);
    iced::application(
        move || Prompt {
            answers: match &request.kind {
                Kind::Questions(questions) => vec![Answer::default(); questions.len()],
                Kind::Tool(_) | Kind::Plan(_) => Vec::new(),
            },
            request: request.clone(),
            colors,
            desktop: desktop::detect(),
            error: None,
            feedback: None,
            question: 0,
            card_height: INITIAL_HEIGHT,
            window: initial,
            spot,
        },
        Prompt::update,
        Prompt::view,
    )
    .subscription(|_| {
        Subscription::batch([
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            keyboard::escape().map(|()| Message::Close),
        ])
    })
    .title(PERMISSION_TITLE)
    .theme(style::theme(appearance))
    .window(window_settings(initial, spot))
    .run()
    .context(WindowSnafu)
}

fn read_request() -> Result<Option<Request>, PermissionError> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_MESSAGE_BYTES)
        .read_to_end(&mut input)
        .context(StdinSnafu)?;
    let event: AgentEvent = serde_json::from_slice(&input).context(RequestSnafu)?;
    Ok(Request::parse(&event))
}

const fn width(kind: &Kind) -> f32 {
    match kind {
        Kind::Plan(_) => PLAN_WIDTH,
        Kind::Tool(_) | Kind::Questions(_) => COMPACT_WIDTH,
    }
}

fn window_settings(size: Size, spot: Option<PromptSpot>) -> window::Settings {
    window::Settings {
        size,
        position: spot.map_or(window::Position::Default, |spot| {
            window::Position::Specific(top_left(spot, size))
        }),
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

fn top_left(spot: PromptSpot, size: Size) -> Point {
    let edge = f64::from(spot.edge) as f32;
    Point::new(
        if spot.left { edge - size.width } else { edge },
        f64::from(spot.middle) as f32 - size.height / 2.0,
    )
}

fn pin(size: Size, spot: Option<PromptSpot>) -> Task<Message> {
    window::latest().and_then(move |id| {
        window::set_max_size(id, None)
            .chain(window::set_min_size(id, None))
            .chain(window::resize(id, size))
            .chain(window::set_min_size(id, Some(size)))
            .chain(window::set_max_size(id, Some(size)))
            .chain(spot.map_or_else(Task::none, |spot| window::move_to(id, top_left(spot, size))))
    })
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

fn deny(message: String) -> Task<Message> {
    decide(&PermissionDecision::Deny { message })
}

fn scroll_direction(visible: bool) -> Direction {
    Direction::Vertical(if visible {
        Scrollbar::new()
            .width(SCROLLBAR_WIDTH)
            .scroller_width(SCROLLBAR_WIDTH)
    } else {
        Scrollbar::hidden()
    })
}

impl Prompt {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Allow => decide(&self.request.allow()),
            Message::Deny => deny(DENY_MESSAGE.to_owned()),
            Message::Suggest(index) => self
                .request
                .suggestions
                .get(index)
                .map_or_else(Task::none, |suggestion| {
                    decide(&self.request.accept(suggestion))
                }),
            Message::GoToTerminal => self.go_to_terminal(),
            Message::Close => iced::exit(),
            Message::OpenFeedback => {
                self.feedback = Some(String::new());
                operation::focus(TEXT_INPUT_ID)
            }
            Message::CloseFeedback => {
                self.feedback = None;
                Task::none()
            }
            Message::FeedbackChanged(feedback) => {
                self.feedback = Some(feedback);
                Task::none()
            }
            Message::SendFeedback => match self.feedback.as_deref().map(str::trim) {
                Some(feedback) if !feedback.is_empty() => deny(feedback.to_owned()),
                _ => Task::none(),
            },
            Message::Toggle(option) => {
                let multi = self.current_question().is_some_and(|q| q.multi_select);
                if let Some(answer) = self.answers.get_mut(self.question) {
                    toggle(answer, option, multi);
                }
                Task::none()
            }
            Message::ToggleOther => {
                let multi = self.current_question().is_some_and(|q| q.multi_select);
                let Some(answer) = self.answers.get_mut(self.question) else {
                    return Task::none();
                };
                toggle_other(answer, multi);
                if answer.other.is_some() {
                    operation::focus(TEXT_INPUT_ID)
                } else {
                    Task::none()
                }
            }
            Message::OtherChanged(other) => {
                if let Some(answer) = self.answers.get_mut(self.question) {
                    answer.other = Some(other);
                }
                Task::none()
            }
            Message::Next => self.next(),
            Message::Back => {
                self.question = self.question.saturating_sub(1);
                Task::none()
            }
            Message::CardSized(size) => {
                self.card_height = size.height;
                self.fit()
            }
            Message::WindowResized(size) => {
                if (size.width - self.window.width).abs() < 1.0
                    && (size.height - self.window.height).abs() < 1.0
                {
                    return Task::none();
                }
                pin(self.window, self.spot)
            }
        }
    }

    fn next(&mut self) -> Task<Message> {
        if !self.current_complete() {
            return Task::none();
        }
        if self.question + 1 < self.answers.len() {
            self.question += 1;
            return Task::none();
        }
        self.request
            .answer(&self.answers)
            .map_or_else(Task::none, |decision| decide(&decision))
    }

    fn current_question(&self) -> Option<&Question> {
        match &self.request.kind {
            Kind::Questions(questions) => questions.get(self.question),
            Kind::Tool(_) | Kind::Plan(_) => None,
        }
    }

    fn current_answer(&self) -> Option<&Answer> {
        self.answers.get(self.question)
    }

    fn current_complete(&self) -> bool {
        self.current_question()
            .zip(self.current_answer())
            .and_then(|(question, answer)| request::answer_text(question, answer))
            .is_some()
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
        let size = Size::new(
            width(&self.request.kind),
            self.card_height
                .min(f32::from(PERMISSION_MAX_HEIGHT))
                .ceil(),
        );
        if size == self.window {
            return Task::none();
        }
        self.window = size;
        pin(size, self.spot)
    }

    fn view(&self) -> Element<'_, Message> {
        let card = column![self.header()]
            .push((!self.request.session.is_empty()).then(|| self.session()))
            .push(self.body())
            .push(self.actions())
            .push(self.suggestions())
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
        .direction(scroll_direction(false))
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn header(&self) -> Element<'_, Message> {
        let title = match self.request.kind {
            Kind::Tool(_) => PERMISSION_TITLE,
            Kind::Plan(_) => PLAN_TITLE,
            Kind::Questions(_) => QUESTION_TITLE,
        };
        let pill = container(
            text(self.request.tool.to_uppercase())
                .size(PILL_SIZE)
                .font(BOLD),
        )
        .padding(PILL_PADDING)
        .style(|_| style::pill(style::tool_color(&self.request.tool)));
        let progress = match &self.request.kind {
            Kind::Questions(questions) if questions.len() > 1 => Some(
                text(format!("{} / {}", self.question + 1, questions.len()))
                    .size(TAG_SIZE)
                    .font(Font::MONOSPACE)
                    .color(style::faded(self.colors.code_text)),
            ),
            Kind::Tool(_) | Kind::Plan(_) | Kind::Questions(_) => None,
        };
        row![
            column![
                text(title)
                    .size(TITLE_SIZE)
                    .font(SEMIBOLD)
                    .color(self.colors.header),
                pill,
            ]
            .spacing(HEADER_GAP),
            space().width(Fill),
        ]
        .push(progress)
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

    fn body(&self) -> Element<'_, Message> {
        match &self.request.kind {
            Kind::Tool(detail) => self.code(detail, TOOL_MAX_HEIGHT),
            Kind::Plan(plan) => column![self.code(plan, PLAN_MAX_HEIGHT)]
                .push(self.feedback.as_deref().map(|feedback| {
                    self.input(FEEDBACK_PLACEHOLDER, feedback)
                        .on_input(Message::FeedbackChanged)
                        .on_submit(Message::SendFeedback)
                }))
                .spacing(CARD_GAP)
                .into(),
            Kind::Questions(_) => self.question_card(),
        }
    }

    fn code<'a>(&self, content: &'a str, max_height: f32) -> Element<'a, Message> {
        let colors = self.colors;
        container(
            scrollable(
                container(
                    text(content)
                        .size(CODE_SIZE)
                        .font(Font::MONOSPACE)
                        .line_height(CODE_LINE_HEIGHT)
                        .wrapping(text::Wrapping::WordOrGlyph)
                        .width(Fill),
                )
                .padding(CODE_PADDING),
            )
            .direction(scroll_direction(true))
            .height(Length::Shrink),
        )
        .max_height(max_height)
        .clip(true)
        .width(Fill)
        .style(move |_| style::code_block(colors))
        .into()
    }

    fn input<'a>(
        &self,
        placeholder: &'a str,
        value: &'a str,
    ) -> text_input::TextInput<'a, Message> {
        let colors = self.colors;
        text_input(placeholder, value)
            .id(TEXT_INPUT_ID)
            .size(SECONDARY_SIZE)
            .padding(INPUT_PADDING)
            .style(move |theme, status| style::input(colors, theme, status))
    }

    fn question_card(&self) -> Element<'_, Message> {
        let (Some(question), Some(answer)) = (self.current_question(), self.current_answer())
        else {
            return space().into();
        };
        let colors = self.colors;
        let hint = if question.multi_select {
            MULTI_HINT
        } else {
            SINGLE_HINT
        };
        let options = question
            .options
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                self.option(
                    &choice.label,
                    choice.description.as_deref(),
                    answer.selected.contains(&index),
                    question.multi_select,
                    Message::Toggle(index),
                )
            })
            .chain([self.option(
                OTHER_LABEL,
                None,
                answer.other.is_some(),
                question.multi_select,
                Message::ToggleOther,
            )]);
        let card = column![]
            .push(question.header.as_deref().map(|header| {
                text(header.to_uppercase())
                    .size(TAG_SIZE)
                    .font(BOLD)
                    .color(colors.code_text)
            }))
            .push(
                text(question.text.trim())
                    .size(QUESTION_SIZE)
                    .color(colors.text),
            )
            .push(
                text(hint)
                    .size(TAG_SIZE)
                    .color(style::faded(colors.code_text)),
            )
            .push(Column::with_children(options).spacing(OPTION_GAP))
            .push(answer.other.as_deref().map(|other| {
                self.input(OTHER_PLACEHOLDER, other)
                    .on_input(Message::OtherChanged)
                    .on_submit(Message::Next)
            }))
            .spacing(OPTION_GAP);
        container(card)
            .padding(QUESTION_PADDING)
            .width(Fill)
            .style(move |_| style::code_block(colors))
            .into()
    }

    fn option<'a>(
        &self,
        label: &'a str,
        description: Option<&'a str>,
        selected: bool,
        multi: bool,
        message: Message,
    ) -> Element<'a, Message> {
        let colors = self.colors;
        let mark = match (multi, selected) {
            (true, true) => CHECKED_MARK,
            (true, false) => UNCHECKED_MARK,
            (false, true) => SELECTED_MARK,
            (false, false) => UNSELECTED_MARK,
        };
        let content = column![text(label).size(SECONDARY_SIZE).font(MEDIUM)]
            .push(description.map(|description| {
                text(description)
                    .size(TAG_SIZE)
                    .color(style::faded(colors.code_text))
            }))
            .width(Fill);
        button(row![text(mark).size(SECONDARY_SIZE), content].spacing(OPTION_GAP))
            .width(Fill)
            .padding(OPTION_PADDING)
            .style(move |theme, status| style::option(colors, theme, selected, status))
            .on_press(message)
            .into()
    }

    fn actions(&self) -> Element<'_, Message> {
        let (primary, primary_message, secondary, secondary_message) = match &self.request.kind {
            Kind::Tool(_) => (
                ALLOW_LABEL,
                Some(Message::Allow),
                DENY_LABEL,
                Some(Message::Deny),
            ),
            Kind::Plan(_) => self.feedback.as_deref().map_or(
                (
                    APPROVE_LABEL,
                    Some(Message::Allow),
                    REJECT_LABEL,
                    Some(Message::Deny),
                ),
                |feedback| {
                    (
                        SEND_LABEL,
                        (!feedback.trim().is_empty()).then_some(Message::SendFeedback),
                        BACK_LABEL,
                        Some(Message::CloseFeedback),
                    )
                },
            ),
            Kind::Questions(questions) => (
                if self.question + 1 < questions.len() {
                    NEXT_LABEL
                } else {
                    SUBMIT_LABEL
                },
                self.current_complete().then_some(Message::Next),
                BACK_LABEL,
                (self.question > 0).then_some(Message::Back),
            ),
        };
        let colors = self.colors;
        let action = |label: &'static str| {
            button(
                text(label)
                    .size(BUTTON_SIZE)
                    .font(SEMIBOLD)
                    .center()
                    .width(Fill),
            )
            .width(Fill)
            .padding(BUTTON_PADDING)
        };
        column![
            row![
                action(primary)
                    .style(style::allow)
                    .on_press_maybe(primary_message),
                action(secondary)
                    .style(move |_, status| style::deny(colors, status))
                    .on_press_maybe(secondary_message),
            ]
            .spacing(CARD_GAP)
        ]
        .push(
            (matches!(self.request.kind, Kind::Plan(_)) && self.feedback.is_none()).then(|| {
                self.secondary_button(
                    FEEDBACK_LABEL,
                    SEMIBOLD,
                    self.colors.secondary_hover_text,
                    Message::OpenFeedback,
                )
            }),
        )
        .spacing(CARD_GAP)
        .into()
    }

    fn suggestions(&self) -> Option<Element<'_, Message>> {
        if self.feedback.is_some() || self.request.suggestions.is_empty() {
            return None;
        }
        let colors = self.colors;
        let buttons = self
            .request
            .suggestions
            .iter()
            .enumerate()
            .map(|(index, suggestion)| {
                button(
                    text(&suggestion.label)
                        .size(SECONDARY_SIZE)
                        .font(MEDIUM)
                        .align_x(Horizontal::Left)
                        .wrapping(text::Wrapping::None)
                        .width(Fill),
                )
                .width(Fill)
                .padding(SECONDARY_PADDING)
                .style(move |_, status| style::secondary(colors, colors.secondary_text, status))
                .on_press(Message::Suggest(index))
                .into()
            });
        Some(Column::with_children(buttons).spacing(OPTION_GAP).into())
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

fn toggle(answer: &mut Answer, option: usize, multi: bool) {
    if !multi {
        answer.selected = vec![option];
        answer.other = None;
    } else if let Some(position) = answer.selected.iter().position(|&index| index == option) {
        answer.selected.remove(position);
    } else {
        answer.selected.push(option);
    }
}

fn toggle_other(answer: &mut Answer, multi: bool) {
    if !multi {
        answer.selected.clear();
        answer.other = Some(answer.other.take().unwrap_or_default());
    } else if answer.other.is_some() {
        answer.other = None;
    } else {
        answer.other = Some(String::new());
    }
}
