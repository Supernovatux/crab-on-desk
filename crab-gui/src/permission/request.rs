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

use std::{collections::HashSet, path::Path};

use crab_common::agent::{AgentEvent, EventKind, PermissionDecision};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::detail;

const PLAN_TOOL: &str = "ExitPlanMode";
const QUESTION_TOOL: &str = "AskUserQuestion";
const PLAN_KEY: &str = "plan";
const ANSWERS_KEY: &str = "answers";
const ANSWER_SEPARATOR: &str = ", ";
const SESSION_SEPARATOR: &str = " \u{b7} ";
const SESSION_ID_CHARS: usize = 3;
const RULE_LABEL_CHARS: usize = 30;
const ELLIPSIS: char = '\u{2026}';
const DIRECTORY_WILDCARD: &str = "**";
const ACCEPT_EDITS_MODE: &str = "acceptEdits";
const PLAN_MODE: &str = "plan";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub tool: String,
    pub session: String,
    pub terminal: Option<u32>,
    pub kind: Kind,
    pub suggestions: Vec<Suggestion>,
    input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Tool(String),
    Plan(String),
    Questions(Vec<Question>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    #[serde(rename = "question")]
    pub text: String,
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Choice {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    permission: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    pub selected: Vec<usize>,
    pub other: Option<String>,
}

#[derive(Deserialize)]
struct QuestionsInput {
    questions: Vec<Question>,
}

impl Request {
    #[must_use]
    pub fn parse(event: &AgentEvent) -> Option<Self> {
        let EventKind::PermissionRequest {
            tool_input,
            suggestions,
        } = &event.kind
        else {
            return None;
        };
        let tool = event.tool_name.clone().unwrap_or_default();
        let kind = match tool.as_str() {
            QUESTION_TOOL => Kind::Questions(questions(tool_input)?),
            PLAN_TOOL => Kind::Plan(
                tool_input
                    .get(PLAN_KEY)
                    .and_then(Value::as_str)
                    .map_or_else(|| detail::describe(&tool, tool_input), str::to_owned),
            ),
            _ => Kind::Tool(detail::describe(&tool, tool_input)),
        };
        let suggestions = match kind {
            Kind::Questions(_) => Vec::new(),
            Kind::Tool(_) | Kind::Plan(_) => labelled(suggestions),
        };
        Some(Self {
            session: session_tag(event),
            terminal: event.source_pid,
            tool,
            kind,
            suggestions,
            input: tool_input.clone(),
        })
    }

    #[must_use]
    pub fn allow(&self) -> PermissionDecision {
        self.allow_with(Vec::new())
    }

    #[must_use]
    pub fn accept(&self, suggestion: &Suggestion) -> PermissionDecision {
        self.allow_with(vec![suggestion.permission.clone()])
    }

    #[must_use]
    pub fn answer(&self, answers: &[Answer]) -> Option<PermissionDecision> {
        let Kind::Questions(questions) = &self.kind else {
            return None;
        };
        let texts = questions
            .iter()
            .zip(answers)
            .map(|(question, answer)| {
                answer_text(question, answer)
                    .map(|text| (question.text.clone(), Value::String(text)))
            })
            .collect::<Option<Map<_, _>>>()?;
        if texts.len() != questions.len() {
            return None;
        }
        let mut input = self.input.as_object().cloned().unwrap_or_default();
        input.insert(ANSWERS_KEY.to_owned(), Value::Object(texts));
        Some(PermissionDecision::Allow {
            updated_input: Some(Value::Object(input)),
            updated_permissions: Vec::new(),
        })
    }

    fn allow_with(&self, updated_permissions: Vec<Value>) -> PermissionDecision {
        PermissionDecision::Allow {
            updated_input: matches!(self.kind, Kind::Plan(_)).then(|| self.input.clone()),
            updated_permissions,
        }
    }
}

#[must_use]
pub fn answer_text(question: &Question, answer: &Answer) -> Option<String> {
    let chosen = question
        .options
        .iter()
        .enumerate()
        .filter(|(index, _)| answer.selected.contains(index))
        .map(|(_, choice)| choice.label.trim().to_owned());
    let other = answer.other.as_deref().map(str::trim);
    if other == Some("") {
        return None;
    }
    let parts = chosen
        .chain(other.map(str::to_owned))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(ANSWER_SEPARATOR))
}

fn questions(input: &Value) -> Option<Vec<Question>> {
    let QuestionsInput { questions } = serde_json::from_value(input.clone()).ok()?;
    let mut keys = HashSet::new();
    let valid = !questions.is_empty()
        && questions
            .iter()
            .all(|question| !question.text.trim().is_empty() && keys.insert(&question.text));
    valid.then_some(questions)
}

fn labelled(suggestions: &[Value]) -> Vec<Suggestion> {
    let mut seen = HashSet::new();
    suggestions
        .iter()
        .filter_map(|permission| {
            let label = suggestion_label(permission)?;
            seen.insert(label.clone()).then(|| Suggestion {
                label,
                permission: permission.clone(),
            })
        })
        .collect()
}

fn suggestion_label(suggestion: &Value) -> Option<String> {
    match suggestion.get("type")?.as_str()? {
        "setMode" => Some(match suggestion.get("mode")?.as_str()? {
            ACCEPT_EDITS_MODE => "Auto-accept edits".to_owned(),
            PLAN_MODE => "Switch to plan mode".to_owned(),
            mode => format!("Switch to {mode} mode"),
        }),
        "addRules" => Some(rule_label(suggestion)),
        _ => None,
    }
}

fn rule_label(suggestion: &Value) -> String {
    let rule = suggestion
        .get("rules")
        .and_then(Value::as_array)
        .and_then(|rules| rules.first())
        .unwrap_or(suggestion);
    let field = |key: &str| {
        rule.get(key)
            .or_else(|| suggestion.get(key))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    let Some(content) = field("ruleContent") else {
        return "Always allow".to_owned();
    };
    if let Some((prefix, _)) = content.split_once(DIRECTORY_WILDCARD) {
        let directory = Path::new(prefix.trim_end_matches(['/', '\\']))
            .file_name()
            .map_or_else(
                || content.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
        return format!(
            "Allow {} in {directory}/",
            field("toolName").unwrap_or_default()
        );
    }
    format!("Always allow `{}`", shorten(content, RULE_LABEL_CHARS))
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars()
        .take(max.saturating_sub(1))
        .chain([ELLIPSIS])
        .collect()
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

#[cfg(test)]
mod tests {
    use crab_common::agent::Agent;
    use serde_json::json;

    use super::*;

    fn event(tool: &str, tool_input: Value, suggestions: Vec<Value>) -> AgentEvent {
        AgentEvent {
            agent: Agent::Claude,
            session_id: "abc123".to_owned(),
            kind: EventKind::PermissionRequest {
                tool_input,
                suggestions,
            },
            cwd: Some("/home/me/project".into()),
            tool_name: Some(tool.to_owned()),
            source_pid: None,
            agent_pid: None,
        }
    }

    #[test]
    fn suggestions_are_labelled_deduplicated_and_unknown_types_dropped() {
        let request = Request::parse(&event(
            "Bash",
            json!({"command": "npm test"}),
            vec![
                json!({"type": "setMode", "mode": "auto", "destination": "session"}),
                json!({"type": "setMode", "mode": "acceptEdits", "destination": "session"}),
                json!({"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "npm test"}]}),
                json!({"type": "addRules", "rules": [{"toolName": "Read", "ruleContent": "/home/me/src/**"}]}),
                json!({"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "npm test"}]}),
                json!({"type": "addDirectories", "directories": ["/tmp"]}),
            ],
        ));
        let labels = request.map(|request| {
            request
                .suggestions
                .into_iter()
                .map(|s| s.label)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            labels,
            Some(vec![
                "Switch to auto mode".to_owned(),
                "Auto-accept edits".to_owned(),
                "Always allow `npm test`".to_owned(),
                "Allow Read in src/".to_owned(),
            ])
        );
    }

    #[test]
    fn accepting_a_suggestion_returns_it_verbatim() {
        let mode = json!({"type": "setMode", "mode": "auto", "destination": "session"});
        let request = Request::parse(&event("Bash", json!({"command": "ls"}), vec![mode.clone()]));
        let decision = request
            .as_ref()
            .and_then(|request| Some(request.accept(request.suggestions.first()?)));
        assert_eq!(
            decision,
            Some(PermissionDecision::Allow {
                updated_input: None,
                updated_permissions: vec![mode],
            })
        );
    }

    #[test]
    fn plan_approval_echoes_the_exact_input() {
        let input = json!({"plan": "# Plan\n1. do it", "planFilePath": "/tmp/p.md"});
        let request = Request::parse(&event("ExitPlanMode", input.clone(), Vec::new()));
        assert_eq!(
            request.as_ref().map(|request| &request.kind),
            Some(&Kind::Plan("# Plan\n1. do it".to_owned()))
        );
        assert_eq!(
            request.map(|request| request.allow()),
            Some(PermissionDecision::Allow {
                updated_input: Some(input),
                updated_permissions: Vec::new(),
            })
        );
    }

    #[test]
    fn answers_are_keyed_by_the_exact_question() {
        let input = json!({"questions": [
            {"question": "Which? ", "header": "Pick", "options": [{"label": "A"}, {"label": "B"}]},
            {"question": "Also?", "multiSelect": true, "options": [{"label": "X"}, {"label": "Y"}]},
        ]});
        let request = Request::parse(&event("AskUserQuestion", input.clone(), Vec::new()));
        let answers = [
            Answer {
                selected: vec![1],
                other: None,
            },
            Answer {
                selected: vec![0, 1],
                other: Some(" Z ".to_owned()),
            },
        ];
        let mut expected = input;
        if let Some(object) = expected.as_object_mut() {
            object.insert(
                ANSWERS_KEY.to_owned(),
                json!({"Which? ": "B", "Also?": "X, Y, Z"}),
            );
        }
        assert_eq!(
            request.and_then(|request| request.answer(&answers)),
            Some(PermissionDecision::Allow {
                updated_input: Some(expected),
                updated_permissions: Vec::new(),
            })
        );
    }

    #[test]
    fn incomplete_answers_are_rejected() {
        let input = json!({"questions": [{"question": "Which?", "options": [{"label": "A"}]}]});
        let request = Request::parse(&event("AskUserQuestion", input, Vec::new()));
        let empty_other = [Answer {
            selected: Vec::new(),
            other: Some("  ".to_owned()),
        }];
        assert_eq!(
            request
                .as_ref()
                .and_then(|request| request.answer(&[Answer::default()])),
            None
        );
        assert_eq!(
            request.and_then(|request| request.answer(&empty_other)),
            None
        );
    }

    #[test]
    fn malformed_questions_fall_back_to_the_terminal() {
        let duplicate = json!({"questions": [{"question": "Q"}, {"question": "Q"}]});
        assert_eq!(
            Request::parse(&event("AskUserQuestion", duplicate, Vec::new())),
            None
        );
        assert_eq!(
            Request::parse(&event(
                "AskUserQuestion",
                json!({"questions": []}),
                Vec::new()
            )),
            None
        );
    }
}
