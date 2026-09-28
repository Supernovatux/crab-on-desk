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
mod job;
mod style;
mod view;

use crab_common::{
    config::Config,
    control::{Control, Message as WidgetMessage},
    dirs::get_config_file,
    gui::{GENERATE_COMMAND, IMPORT_COMMAND, SETTINGS_APP_ID},
    hooks, ipc, toml_file,
};
use iced::{Size, Task, window};
use snafu::Snafu;

use crate::{
    desktop::{self, Desktop, Output},
    theme,
};
use catalog::ThemeEntry;
use style::Colors;

const TITLE: &str = "Crab Settings";
const WINDOW_SIZE: Size = Size::new(800.0, 560.0);
const MIN_WINDOW_SIZE: Size = Size::new(640.0, 480.0);
const SAVED_OFFLINE: &str = "Saved. The widget is not running; it applies at the next start.";
const SAVED: &str = "Saved. The widget is restarting.";
const DONE: &str = "Done.";

#[derive(Debug, Snafu)]
pub enum SettingsError {
    #[snafu(display("Unable to show the settings window"))]
    Window { source: iced::Error },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
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
    SelectTheme(String),
    Regenerate(String),
    Delete(String),
    ConfirmDelete(String),
    ReferenceChanged(String),
    Import,
    JobFinished(Result<(), String>),
    Hooks(bool),
    MoveTo(String),
    DismissToast,
}

#[derive(Debug, Clone)]
enum Hooks {
    Installed,
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    error: bool,
}

struct Settings {
    page: Page,
    colors: &'static Colors,
    config: Option<Config>,
    themes: Vec<ThemeEntry>,
    hooks: Hooks,
    desktop: Option<Box<dyn Desktop>>,
    outputs: Result<Vec<Output>, String>,
    reference: String,
    job: Option<String>,
    deleting: Option<String>,
    toast: Option<Toast>,
}

pub fn run() -> Result<(), SettingsError> {
    let appearance = theme::system();
    let colors = style::colors(appearance);
    iced::application(
        move || Settings::new(colors),
        Settings::update,
        Settings::view,
    )
    .title(TITLE)
    .theme(style::theme(appearance))
    .window(window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(MIN_WINDOW_SIZE),
        platform_specific: platform_settings(),
        ..window::Settings::default()
    })
    .run()
    .map_err(|source| SettingsError::Window { source })
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

impl Settings {
    fn new(colors: &'static Colors) -> Self {
        let desktop = desktop::detect();
        let mut settings = Self {
            page: Page::General,
            colors,
            config: None,
            themes: Vec::new(),
            hooks: Hooks::Missing,
            outputs: Ok(Vec::new()),
            desktop,
            reference: String::new(),
            job: None,
            deleting: None,
            toast: None,
        };
        match get_config_file()
            .map_err(report)
            .and_then(|path| toml_file::read(path).map_err(report))
        {
            Ok(config) => settings.config = Some(config),
            Err(error) => settings.fail(error),
        }
        settings.refresh_themes();
        settings.refresh_hooks();
        settings.refresh_outputs();
        settings
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show(page) => {
                self.page = page;
                self.deleting = None;
                if page == Page::Displays {
                    self.refresh_outputs();
                }
            }
            Message::FreeRoam(enabled) => self.change_config(|config| config.free_roam = enabled),
            Message::SelectTheme(theme) => {
                self.change_config(|config| config.default_theme = theme);
            }
            Message::Regenerate(theme) => {
                return self.start_job(
                    format!("Generating {theme}…"),
                    vec![GENERATE_COMMAND.to_owned(), theme],
                );
            }
            Message::ConfirmDelete(theme) => self.deleting = Some(theme),
            Message::Delete(theme) => self.delete(&theme),
            Message::ReferenceChanged(reference) => self.reference = reference,
            Message::Import => {
                let reference = self.reference.trim().to_owned();
                return self.start_job(
                    "Importing themes, this takes several minutes…".to_owned(),
                    vec![IMPORT_COMMAND.to_owned(), reference],
                );
            }
            Message::JobFinished(result) => {
                self.job = None;
                self.refresh_themes();
                match result {
                    Ok(()) => self.apply(DONE, DONE),
                    Err(error) => self.fail(error),
                }
            }
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
        }
        Task::none()
    }

    fn start_job(&mut self, label: String, args: Vec<String>) -> Task<Message> {
        if self.job.is_some() {
            return Task::none();
        }
        self.job = Some(label);
        self.toast = None;
        Task::perform(job::run_settings(args), Message::JobFinished)
    }

    fn change_config(&mut self, change: impl FnOnce(&mut Config)) {
        let Some(config) = self.config.as_mut() else {
            return;
        };
        change(config);
        let saved = get_config_file()
            .map_err(report)
            .and_then(|path| toml_file::write(path, &*config).map_err(report));
        match saved {
            Ok(()) => self.apply(SAVED, SAVED_OFFLINE),
            Err(error) => self.fail(error),
        }
    }

    fn apply(&mut self, online: &str, offline: &str) {
        match notify_widget(&WidgetMessage::Control(Control::Reload)) {
            Ok(true) => self.say(online),
            Ok(false) => self.say(offline),
            Err(error) => self.fail(error),
        }
    }

    fn delete(&mut self, theme: &str) {
        self.deleting = None;
        if self.is_active(theme) {
            return;
        }
        match catalog::delete(theme) {
            Ok(()) => self.say(format!("Deleted {theme}.")),
            Err(error) => self.fail(report(error)),
        }
        self.refresh_themes();
    }

    fn is_active(&self, theme: &str) -> bool {
        self.config
            .as_ref()
            .is_some_and(|config| config.default_theme == theme)
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
        if let Some(desktop) = &self.desktop {
            self.outputs = desktop.outputs().map_err(report);
        }
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
