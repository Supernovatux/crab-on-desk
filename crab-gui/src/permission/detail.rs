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

#[must_use]
pub fn describe(tool: &str, tool_input: &Value) -> String {
    let pretty = || serde_json::to_string_pretty(tool_input).unwrap_or_default();
    let Value::Object(input) = tool_input else {
        return pretty();
    };
    [
        (SHELL_TOOLS.as_slice(), COMMAND_KEYS.as_slice()),
        (FILE_TOOLS.as_slice(), PATH_KEYS.as_slice()),
        (SEARCH_TOOLS.as_slice(), PATTERN_KEYS.as_slice()),
    ]
    .into_iter()
    .find(|(tools, _)| is_one_of(tool, tools))
    .and_then(|(_, keys)| first(input, keys))
    .unwrap_or_else(pretty)
}

fn is_one_of(tool: &str, tools: &[&str]) -> bool {
    let tool = tool.trim();
    tools
        .iter()
        .any(|candidate| tool.eq_ignore_ascii_case(candidate))
}

fn first(input: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        input
            .get(*key)?
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bash_shows_the_command() {
        let input = json!({"command": "cargo build", "description": "Build it"});
        assert_eq!(describe("Bash", &input), "cargo build");
    }

    #[test]
    fn edit_shows_the_file_path() {
        let input = json!({"file_path": "/a/b.rs", "old_string": "x"});
        assert_eq!(describe("Edit", &input), "/a/b.rs");
    }

    #[test]
    fn unknown_tool_shows_pretty_json() {
        let input = json!({"url": "https://example.com", "n": 1});
        assert_eq!(
            describe("WebFetch", &input),
            "{\n  \"url\": \"https://example.com\",\n  \"n\": 1\n}"
        );
    }
}
