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

use iced::{
    Alignment, ContentFit, Element, Fill, Length,
    widget::{
        Column, button, column, container, grid, image, mouse_area, row, rule, scrollable,
        scrollable::{Direction, Scrollbar},
        space, text, toggler,
    },
};

use super::{Hooks, Message, Mode, Page, Settings, catalog::ThemeEntry, style, style::Tone};
use crate::theme::{BOLD, MEDIUM, SEMIBOLD};

const SIDEBAR_WIDTH: f32 = 200.0;
const SIDEBAR_PADDING: [u16; 2] = [16, 8];
const SIDEBAR_ITEM_PADDING: [u16; 2] = [8, 12];
const SIDEBAR_GAP: u32 = 4;
const CONTENT_PADDING: [u16; 2] = [28, 36];
const TITLE_GAP: u32 = 4;
const HEADER_GAP: u32 = 24;
const SECTION_GAP: u32 = 28;
const SECTION_TITLE_GAP: u32 = 8;
const SECTION_TITLE_PADDING: [u16; 2] = [0, 4];
const ROW_PADDING: [u16; 2] = [12, 16];
const ROW_GAP: u32 = 16;
const ROW_TEXT_GAP: u32 = 2;
const CARD_GAP: u32 = 14;
const CARD_PADDING: u16 = 12;
const CARD_INNER_GAP: u32 = 8;
const CARD_MAX_WIDTH: f32 = 264.0;
const THUMB_HEIGHT: f32 = 150.0;
const BADGE_PADDING: [u16; 2] = [1, 6];
const BUTTON_PADDING: [u16; 2] = [7, 10];
const TOAST_PADDING: [u16; 2] = [10, 14];
const TOAST_MARGIN: u16 = 16;
const SWITCH_SIZE: f32 = 24.0;
const SCROLLBAR_WIDTH: f32 = 6.0;

const BODY_SIZE: u32 = 13;
const TITLE_SIZE: u32 = 22;
const SUBTITLE_SIZE: u32 = 12;
const SECTION_SIZE: u32 = 11;
const DESCRIPTION_SIZE: u32 = 11;
const BUTTON_SIZE: u32 = 12;
const BADGE_SIZE: u32 = 10;

const SETUP_TITLE: &str = "Welcome to Crab on Desk";
const SETUP_SUBTITLE: &str =
    "Pick a theme to get started. Everything here can be changed later in Settings.";
const NO_THEMES: &str =
    "Install the crab-on-desk-themes package, or run make themes-install from a checkout.";

impl Page {
    const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Theme => "Theme",
            Self::Claude => "Claude Code",
            Self::Displays => "Displays",
        }
    }

    const fn subtitle(self) -> &'static str {
        match self {
            Self::General => "How the crab behaves on your desktop.",
            Self::Theme => "Choose and regenerate the crab's look.",
            Self::Claude => "Hooks that let Claude Code drive the crab and ask for permissions.",
            Self::Displays => "Choose which screen the crab lives on.",
        }
    }
}

impl Settings {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let (title, subtitle, body) = match (self.mode, self.page) {
            (Mode::Setup, _) => (SETUP_TITLE, SETUP_SUBTITLE, self.setup()),
            (Mode::Settings, Page::General) => {
                (self.page.label(), self.page.subtitle(), self.general())
            }
            (Mode::Settings, Page::Theme) => {
                (self.page.label(), self.page.subtitle(), self.themes())
            }
            (Mode::Settings, Page::Claude) => {
                (self.page.label(), self.page.subtitle(), self.claude())
            }
            (Mode::Settings, Page::Displays) => {
                (self.page.label(), self.page.subtitle(), self.displays())
            }
        };
        let content = column![
            text(title)
                .size(TITLE_SIZE)
                .font(BOLD)
                .color(self.colors.text),
            text(subtitle)
                .size(SUBTITLE_SIZE)
                .color(self.colors.text_secondary),
        ]
        .spacing(TITLE_GAP);
        let page = scrollable(
            column![content, body]
                .spacing(HEADER_GAP)
                .padding(CONTENT_PADDING),
        )
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH),
        ))
        .height(Fill);
        let colors = self.colors;
        let panel = container(column![page].push(self.toast()))
            .width(Fill)
            .height(Fill)
            .style(move |_| style::panel(colors));
        match self.mode {
            Mode::Setup => panel.into(),
            Mode::Settings => row![self.sidebar(), panel].into(),
        }
    }

    fn setup(&self) -> Element<'_, Message> {
        let finish = row![
            space().width(Fill),
            self.soft(
                "Finish",
                Tone::Accent,
                self.theme.is_some().then_some(Message::Finish),
            ),
        ];
        column![self.themes(), self.general(), self.claude(), finish]
            .spacing(SECTION_GAP)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let items = Page::ALL.into_iter().map(|page| {
            let active = page == self.page;
            button(text(page.label()).size(BODY_SIZE).font(MEDIUM))
                .width(Fill)
                .padding(SIDEBAR_ITEM_PADDING)
                .style(move |_, status| style::sidebar_item(colors, active, status))
                .on_press(Message::Show(page))
                .into()
        });
        container(Column::with_children(items).spacing(SIDEBAR_GAP))
            .width(SIDEBAR_WIDTH)
            .height(Fill)
            .padding(SIDEBAR_PADDING)
            .style(move |_| style::sidebar(colors))
            .into()
    }

    fn general(&self) -> Element<'_, Message> {
        let colors = self.colors;
        section(
            self,
            "Behaviour",
            vec![
                self.row(
                    "Free roam",
                    Some("Walk around the screen while idle."),
                    toggler(self.free_roam)
                        .size(SWITCH_SIZE)
                        .on_toggle(Message::FreeRoam)
                        .style(move |theme, status| style::switch(colors, theme, status))
                        .into(),
                ),
            ],
        )
    }

    fn themes(&self) -> Element<'_, Message> {
        if self.themes.is_empty() {
            section(
                self,
                "Themes",
                vec![self.row("No themes installed", Some(NO_THEMES), space().into())],
            )
        } else {
            column![
                section_title(self, "Themes"),
                grid(self.themes.iter().map(|theme| self.theme_card(theme)))
                    .spacing(CARD_GAP)
                    .fluid(CARD_MAX_WIDTH)
                    .height(Length::Shrink),
            ]
            .spacing(SECTION_TITLE_GAP)
            .into()
        }
    }

    fn theme_card<'a>(&'a self, theme: &'a ThemeEntry) -> Element<'a, Message> {
        let colors = self.colors;
        let active = self.is_active(&theme.name);
        let thumbnail: Element<'a, Message> = theme.thumbnail.as_ref().map_or_else(
            || space().into(),
            |handle| {
                image(handle.clone())
                    .content_fit(ContentFit::Contain)
                    .width(Fill)
                    .height(Fill)
                    .into()
            },
        );
        let badge = active.then_some(("Active", Tone::Accent));
        let name = row![
            text(&theme.name)
                .size(BODY_SIZE)
                .font(SEMIBOLD)
                .color(colors.text)
        ]
        .push(badge.map(|(label, tone)| {
            container(text(label).size(BADGE_SIZE).font(SEMIBOLD))
                .padding(BADGE_PADDING)
                .style(move |theme| style::badge(colors, theme, tone))
        }))
        .spacing(CARD_INNER_GAP / 4 * 3)
        .align_y(Alignment::Center);
        let preview = button(
            column![
                container(thumbnail)
                    .width(Fill)
                    .height(THUMB_HEIGHT)
                    .style(move |_| style::thumb(colors)),
                name,
            ]
            .spacing(CARD_INNER_GAP),
        )
        .padding(0)
        .style(|_, _| button::Style::default())
        .on_press_maybe((!active).then(|| Message::SelectTheme(theme.name.clone())));
        container(preview)
            .padding(CARD_PADDING)
            .width(Fill)
            .style(move |theme| style::card(colors, theme, active))
            .into()
    }

    fn claude(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let (description, installed) = match &self.hooks {
            Hooks::Installed => (
                "Installed in Claude Code's settings.json.".to_owned(),
                Some(true),
            ),
            Hooks::Missing => (
                "Not installed. Claude Code will not drive the crab.".to_owned(),
                Some(false),
            ),
            Hooks::Unknown(error) => (error.clone(), None),
        };
        let switch = toggler(installed.unwrap_or_default())
            .size(SWITCH_SIZE)
            .on_toggle_maybe(installed.map(|_| Message::Hooks as fn(bool) -> Message))
            .style(move |theme, status| style::switch(colors, theme, status));
        section(
            self,
            "Hooks",
            vec![self.row_owned("Claude Code".to_owned(), Some(description), switch.into())],
        )
    }

    fn displays(&self) -> Element<'_, Message> {
        let rows = match &self.outputs {
            Err(error) => vec![self.row_owned(
                "Unable to list screens".to_owned(),
                Some(error.clone()),
                space().into(),
            )],
            Ok(outputs) => outputs
                .iter()
                .map(|output| {
                    self.row_owned(
                        output.name.clone(),
                        Some(format!(
                            "{} \u{b7} {}\u{d7}{}",
                            output.description, output.width, output.height
                        )),
                        self.soft(
                            "Move crab here",
                            Tone::Plain,
                            Some(Message::MoveTo(output.name.clone())),
                        ),
                    )
                })
                .collect(),
        };
        section(self, "Screens", rows)
    }

    fn toast(&self) -> Option<Element<'_, Message>> {
        let toast = self.toast.as_ref()?;
        let colors = self.colors;
        let error = toast.error;
        let bubble = container(text(&toast.text).size(BUTTON_SIZE))
            .padding(TOAST_PADDING)
            .width(Fill)
            .style(move |_| style::toast(colors, error));
        Some(
            container(mouse_area(bubble).on_press(Message::DismissToast))
                .padding(TOAST_MARGIN)
                .into(),
        )
    }

    fn row<'a>(
        &'a self,
        label: &'a str,
        description: Option<&'a str>,
        control: Element<'a, Message>,
    ) -> Element<'a, Message> {
        self.row_owned(label.to_owned(), description.map(str::to_owned), control)
    }

    fn row_owned<'a>(
        &'a self,
        label: String,
        description: Option<String>,
        control: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let colors = self.colors;
        let labels = column![text(label).size(BODY_SIZE).font(MEDIUM).color(colors.text)]
            .push(description.map(|description| {
                text(description)
                    .size(DESCRIPTION_SIZE)
                    .color(colors.text_secondary)
            }))
            .spacing(ROW_TEXT_GAP)
            .width(Fill);
        row![labels, control]
            .spacing(ROW_GAP)
            .padding(ROW_PADDING)
            .align_y(Alignment::Center)
            .into()
    }

    fn soft<'a>(
        &'a self,
        label: &'a str,
        tone: Tone,
        message: Option<Message>,
    ) -> Element<'a, Message> {
        let colors = self.colors;
        button(text(label).size(BUTTON_SIZE).font(MEDIUM))
            .padding(BUTTON_PADDING)
            .style(move |theme, status| style::soft(colors, theme, tone, status))
            .on_press_maybe(message)
            .into()
    }
}

fn section_title<'a>(settings: &Settings, title: &str) -> Element<'a, Message> {
    container(
        text(title.to_uppercase())
            .size(SECTION_SIZE)
            .font(BOLD)
            .color(settings.colors.text_tertiary),
    )
    .padding(SECTION_TITLE_PADDING)
    .into()
}

fn section<'a>(
    settings: &Settings,
    title: &str,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let colors = settings.colors;
    let mut body = Column::new();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            body = body.push(rule::horizontal(1).style(move |_| style::divider(colors)));
        }
        body = body.push(row);
    }
    column![
        section_title(settings, title),
        container(body)
            .width(Fill)
            .style(move |_| style::rows(colors)),
    ]
    .spacing(SECTION_TITLE_GAP)
    .into()
}
