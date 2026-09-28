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

use serde_json::{Map, Value};

const PREVIEW_CHARS: usize = 120;
const FALLBACK_PREVIEW_CHARS: usize = 100;
const ELLIPSIS: char = '…';
const PLAN_TOOL: &str = "exitplanmode";
const PLAN_KEY: &str = "plan";
const DESCRIPTION_KEY: &str = "description";
const SHELL_TOOLS: [&str; 3] = ["bash", "shell", "run_command"];
const FILE_TOOLS: [&str; 3] = ["edit", "write", "read"];
const SEARCH_TOOLS: [&str; 2] = ["glob", "grep"];
const COMMAND_KEYS: [&str; 4] = ["command", "CommandLine", "Command", "cmd"];
const PATH_KEYS: [&str; 5] = [
    "file_path",
    "filepath",
    "path",
    "AbsolutePath",
    "TargetFile",
];
const PATTERN_KEYS: [&str; 4] = ["pattern", "Pattern", "query", "Query"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub preview: String,
    pub full: String,
    pub truncated: bool,
}

#[must_use]
pub fn describe(tool: &str, tool_input: &str) -> Detail {
    match serde_json::from_str::<Value>(tool_input) {
        Ok(Value::Object(input)) => Detail {
            preview: preview(tool, &input),
            full: full(tool, &input, tool_input),
            truncated: false,
        },
        _ => Detail {
            preview: truncate(tool_input.trim(), FALLBACK_PREVIEW_CHARS),
            full: tool_input.to_owned(),
            truncated: !tool_input.is_empty(),
        },
    }
}

fn preview(tool: &str, input: &Map<String, Value>) -> String {
    let specific = plan(tool, input)
        .or_else(|| string(input, DESCRIPTION_KEY))
        .or_else(|| {
            is_one_of(tool, &SHELL_TOOLS)
                .then(|| string(input, "command"))
                .flatten()
        })
        .or_else(|| {
            is_one_of(tool, &FILE_TOOLS)
                .then(|| string(input, "file_path"))
                .flatten()
        })
        .or_else(|| {
            is_one_of(tool, &SEARCH_TOOLS)
                .then(|| string(input, "pattern"))
                .flatten()
        });
    if let Some(text) = specific {
        return truncate(&text, PREVIEW_CHARS);
    }
    let fallback = input
        .values()
        .find_map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .map_or_else(|| Value::Object(input.clone()).to_string(), str::to_owned);
    truncate(&fallback, FALLBACK_PREVIEW_CHARS)
}

fn full(tool: &str, input: &Map<String, Value>, tool_input: &str) -> String {
    plan(tool, input)
        .or_else(|| {
            is_one_of(tool, &SHELL_TOOLS)
                .then(|| first(input, &COMMAND_KEYS))
                .flatten()
        })
        .or_else(|| {
            is_one_of(tool, &FILE_TOOLS)
                .then(|| first(input, &PATH_KEYS))
                .flatten()
        })
        .or_else(|| {
            is_one_of(tool, &SEARCH_TOOLS)
                .then(|| first(input, &PATTERN_KEYS))
                .flatten()
        })
        .unwrap_or_else(|| tool_input.to_owned())
}

fn plan(tool: &str, input: &Map<String, Value>) -> Option<String> {
    tool.trim()
        .eq_ignore_ascii_case(PLAN_TOOL)
        .then(|| {
            input
                .get(PLAN_KEY)?
                .as_str()
                .map(|plan| plan.trim().to_owned())
        })
        .flatten()
}

fn is_one_of(tool: &str, tools: &[&str]) -> bool {
    let tool = tool.trim();
    tools
        .iter()
        .any(|candidate| tool.eq_ignore_ascii_case(candidate))
}

fn string(input: &Map<String, Value>, key: &str) -> Option<String> {
    input
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn first(input: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| string(input, key))
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars()
        .take(max.saturating_sub(1))
        .chain([ELLIPSIS])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_previews_description_and_details_command() {
        let detail = describe(
            "Bash",
            r#"{"command": "cargo build", "description": "Build it"}"#,
        );
        assert_eq!(detail.preview, "Build it");
        assert_eq!(detail.full, "cargo build");
        assert!(!detail.truncated);
    }

    #[test]
    fn edit_uses_file_path() {
        let detail = describe("Edit", r#"{"file_path": "/a/b.rs", "old_string": "x"}"#);
        assert_eq!(detail.preview, "/a/b.rs");
        assert_eq!(detail.full, "/a/b.rs");
    }

    #[test]
    fn unknown_tool_previews_first_string_and_details_json() {
        let input = "{\n  \"url\": \"https://example.com\",\n  \"n\": 1\n}";
        let detail = describe("WebFetch", input);
        assert_eq!(detail.preview, "https://example.com");
        assert_eq!(detail.full, input);
    }

    #[test]
    fn long_preview_is_truncated_with_ellipsis() {
        let command = "x".repeat(200);
        let detail = describe("Bash", &format!(r#"{{"command": "{command}"}}"#));
        assert_eq!(detail.preview.chars().count(), PREVIEW_CHARS);
        assert!(detail.preview.ends_with(ELLIPSIS));
        assert_eq!(detail.full, command);
    }

    #[test]
    fn cut_off_input_is_marked_truncated() {
        let detail = describe("Write", "{\n  \"content\": \"abc");
        assert!(detail.truncated);
        assert_eq!(detail.full, "{\n  \"content\": \"abc");
    }
}
