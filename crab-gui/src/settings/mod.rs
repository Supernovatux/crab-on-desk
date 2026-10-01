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

mod catalog;
mod style;
mod view;

use std::{fs::File, io, path::PathBuf};

use crab_common::{
    config::Config,
    control::{Control, Message as WidgetMessage},
    dirs::{CommonError, get_settings_lock, get_sibling_executable},
    gui::{SETTINGS_APP_ID, WIDGET_BINARY},
    hooks, ipc, process,
};
use iced::{Size, Subscription, Task, window};
use snafu::{ResultExt, Snafu};

use crate::{
    outputs::{self, Output},
    theme,
};
use catalog::ThemeEntry;
use style::Colors;

const SETTINGS_TITLE: &str = "Crab Settings";
const SETUP_TITLE: &str = "Crab Setup";
const WINDOW_SIZE: Size = Size::new(800.0, 560.0);
const MIN_WINDOW_SIZE: Size = Size::new(640.0, 480.0);
const SAVED_OFFLINE: &str = "Saved. The widget is not running; it applies at the next start.";
const SAVED: &str = "Saved. The widget is restarting.";
const NO_HOOKS: &str =
    "No agent hooks are installed, so nothing will drive the crab. Turn on Claude Code hooks, or press Finish anyway.";

#[derive(Debug, Snafu)]
pub enum SettingsError {
    #[snafu(display("Unable to show the settings window"))]
    Window { source: iced::Error },
    #[snafu(context(false))]
    Dir { source: CommonError },
    #[snafu(display("Unable to lock {path:?}"))]
    Lock { source: io::Error, path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Settings,
    Setup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    General,
    Theme,
    Claude,
    Displays,
}

impl Page {
    const ALL: [Self; 4] = [Self::General, Self::Theme, Self::Claude, Self::Displays];
}

#[derive(Debug, Clone)]
enum Message {
    Show(Page),
    FreeRoam(bool),
    TrackCursor(bool),
    SelectTheme(String),
    Hooks(bool),
    Finish,
    MoveTo(String),
    DismissToast,
    Mapped,
}

#[derive(Debug, Clone)]
enum Hooks {
    Installed,
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HooksWarning {
    Unseen,
    Shown,
}

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    error: bool,
}

struct Settings {
    mode: Mode,
    page: Page,
    colors: &'static Colors,
    theme: Option<String>,
    free_roam: bool,
    track_cursor: bool,
    themes: Vec<ThemeEntry>,
    hooks: Hooks,
    hooks_warning: HooksWarning,
    outputs: Result<Vec<Output>, String>,
    toast: Option<Toast>,
    size: Pin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    Fixed,
    Free,
}

pub fn run(config: Option<Config>, page: Page) -> Result<(), SettingsError> {
    let Some(_lock) = lock()? else {
        return Ok(());
    };
    let appearance = theme::system();
    let colors = style::colors(appearance);
    let title = if config.is_some() {
        SETTINGS_TITLE
    } else {
        SETUP_TITLE
    };
    iced::application(
        move || Settings::new(colors, config.clone(), page),
        Settings::update,
        Settings::view,
    )
    .title(title)
    .theme(style::theme(appearance))
    .subscription(Settings::subscription)
    .window(window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(WINDOW_SIZE),
        max_size: Some(WINDOW_SIZE),
        platform_specific: platform_settings(),
        ..window::Settings::default()
    })
    .run()
    .map_err(|source| SettingsError::Window { source })
}

fn lock() -> Result<Option<File>, SettingsError> {
    let path = get_settings_lock()?;
    process::try_lock(&path).context(LockSnafu { path })
}

#[cfg(target_os = "linux")]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        application_id: SETTINGS_APP_ID.to_owned(),
        ..window::settings::PlatformSpecific::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

fn report(error: impl std::error::Error + 'static) -> String {
    snafu::Report::from_error(error).to_string()
}

fn notify_widget(message: &WidgetMessage) -> Result<bool, String> {
    let Some(mut stream) = ipc::connect().map_err(report)? else {
        return Ok(false);
    };
    ipc::send(&mut stream, message).map_err(report)?;
    Ok(true)
}

fn start_widget() -> Result<(), String> {
    if notify_widget(&WidgetMessage::Control(Control::Reload))? {
        return Ok(());
    }
    let program = get_sibling_executable(WIDGET_BINARY).map_err(report)?;
    process::detached(&program)
        .spawn()
        .map_err(|error| format!("Unable to start {}: {error}", program.display()))?;
    Ok(())
}

impl Settings {
    fn new(colors: &'static Colors, config: Option<Config>, page: Page) -> Self {
        let mode = if config.is_some() {
            Mode::Settings
        } else {
            Mode::Setup
        };
        let mut settings = Self {
            mode,
            page,
            colors,
            free_roam: config.as_ref().is_some_and(|config| config.free_roam),
            track_cursor: config.as_ref().is_none_or(|config| config.track_cursor),
            theme: config.map(|config| config.default_theme),
            themes: Vec::new(),
            hooks: Hooks::Missing,
            hooks_warning: HooksWarning::Unseen,
            outputs: Ok(Vec::new()),
            toast: None,
            size: Pin::Fixed,
        };
        settings.refresh_themes();
        settings.refresh_hooks();
        settings.refresh_outputs();
        settings
    }

    fn subscription(&self) -> Subscription<Message> {
        match self.size {
            Pin::Fixed => window::frames().map(|_| Message::Mapped),
            Pin::Free => Subscription::none(),
        }
    }

    fn unpin(&mut self) -> Task<Message> {
        if self.size == Pin::Free {
            return Task::none();
        }
        self.size = Pin::Free;
        window::latest().and_then(|id| {
            window::set_max_size(id, None).chain(window::set_min_size(id, Some(MIN_WINDOW_SIZE)))
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show(page) => {
                self.page = page;
                if page == Page::Displays {
                    self.refresh_outputs();
                }
            }
            Message::FreeRoam(enabled) => {
                self.free_roam = enabled;
                self.save();
            }
            Message::TrackCursor(enabled) => {
                self.track_cursor = enabled;
                self.save();
            }
            Message::SelectTheme(theme) => {
                self.theme = Some(theme);
                self.save();
            }
            Message::Finish => return self.finish(),
            Message::Hooks(install) => {
                let result = if install {
                    hooks::install()
                } else {
                    hooks::uninstall()
                };
                match result {
                    Ok(()) => self.say(if install {
                        "Claude Code hooks installed."
                    } else {
                        "Claude Code hooks removed."
                    }),
                    Err(error) => self.fail(report(error)),
                }
                self.refresh_hooks();
            }
            Message::MoveTo(output) => {
                match notify_widget(&WidgetMessage::Control(Control::MoveToOutput {
                    output: output.clone(),
                })) {
                    Ok(true) => self.say(format!("Moved the crab to {output}.")),
                    Ok(false) => self.fail("The widget is not running.".to_owned()),
                    Err(error) => self.fail(error),
                }
            }
            Message::DismissToast => self.toast = None,
            Message::Mapped => return self.unpin(),
        }
        Task::none()
    }

    fn config(&self) -> Option<Config> {
        self.theme.clone().map(|default_theme| Config {
            default_theme,
            free_roam: self.free_roam,
            track_cursor: self.track_cursor,
        })
    }

    fn save(&mut self) {
        if self.mode == Mode::Setup {
            return;
        }
        let Some(config) = self.config() else {
            return;
        };
        match config.save() {
            Ok(()) => self.apply(SAVED, SAVED_OFFLINE),
            Err(error) => self.fail(report(error)),
        }
    }

    fn finish(&mut self) -> Task<Message> {
        let Some(config) = self.config() else {
            return Task::none();
        };
        if self.needs_hooks_warning() {
            self.hooks_warning = HooksWarning::Shown;
            self.fail(NO_HOOKS.to_owned());
            return Task::none();
        }
        match config.save().map_err(report).and_then(|()| start_widget()) {
            Ok(()) => iced::exit(),
            Err(error) => {
                self.fail(error);
                Task::none()
            }
        }
    }

    fn needs_hooks_warning(&self) -> bool {
        self.hooks_warning == HooksWarning::Unseen && !self.has_hooks()
    }

    const fn has_hooks(&self) -> bool {
        matches!(self.hooks, Hooks::Installed)
    }

    fn apply(&mut self, online: &str, offline: &str) {
        match notify_widget(&WidgetMessage::Control(Control::Reload)) {
            Ok(true) => self.say(online),
            Ok(false) => self.say(offline),
            Err(error) => self.fail(error),
        }
    }

    fn is_active(&self, theme: &str) -> bool {
        self.theme.as_deref() == Some(theme)
    }

    fn refresh_themes(&mut self) {
        match catalog::list() {
            Ok(themes) => self.themes = themes,
            Err(error) => self.fail(report(error)),
        }
    }

    fn refresh_hooks(&mut self) {
        self.hooks = match hooks::installed() {
            Ok(true) => Hooks::Installed,
            Ok(false) => Hooks::Missing,
            Err(error) => Hooks::Unknown(report(error)),
        };
    }

    fn refresh_outputs(&mut self) {
        self.outputs = outputs::list().map_err(report);
    }

    fn say(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            error: false,
        });
    }

    fn fail(&mut self, text: String) {
        self.toast = Some(Toast { text, error: true });
    }
}
