use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use rand::RngCore;
use serde_json::Value;

use crate::action::{Action, Direction};
use crate::agent::{Agent, Observation};

/// An agent that delegates to an external Python process via stdin/stdout JSON.
///
/// The subprocess is spawned lazily on first `act()` and reused across turns.
/// Communication protocol:
/// - Send `{"type": "reset"}\n` on reset
/// - Send observation JSON (with `"type": "observation"`) + `\n`
/// - Read one line of JSON back: `{"actions": [...]}`
pub struct SubprocessAgent {
    name: String,
    command: String,
    args: Vec<String>,
    child: Option<SubprocessHandle>,
}

struct SubprocessHandle {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

impl SubprocessAgent {
    pub fn new(name: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args,
            child: None,
        }
    }

    fn ensure_started(&mut self) {
        if self.child.is_some() {
            return;
        }
        match Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(mut child) => {
                let stdout = child.stdout.take().expect("stdout piped");
                self.child = Some(SubprocessHandle {
                    child,
                    reader: BufReader::new(stdout),
                });
            }
            Err(e) => {
                eprintln!("Failed to spawn subprocess agent '{}': {}", self.name, e);
            }
        }
    }

    fn send_line(&mut self, line: &str) -> bool {
        if let Some(ref mut handle) = self.child {
            if let Some(ref mut stdin) = handle.child.stdin {
                if writeln!(stdin, "{}", line).is_ok() {
                    return stdin.flush().is_ok();
                }
            }
        }
        false
    }

    fn read_line(&mut self) -> Option<String> {
        if let Some(ref mut handle) = self.child {
            let mut buf = String::new();
            match handle.reader.read_line(&mut buf) {
                Ok(0) => None, // EOF
                Ok(_) => Some(buf),
                Err(_) => None,
            }
        } else {
            None
        }
    }

    fn parse_actions(response: &str) -> Vec<Action> {
        let val: Value = match serde_json::from_str(response) {
            Ok(v) => v,
            Err(_) => return vec![],
        };

        let actions_arr = match val.get("actions").and_then(|a| a.as_array()) {
            Some(a) => a,
            None => return vec![],
        };

        let mut actions = Vec::new();
        for action in actions_arr {
            if action.is_string() && action.as_str() == Some("Pass") {
                actions.push(Action::Pass);
                continue;
            }
            if let Some(mv) = action.get("Move") {
                let row = mv.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = mv.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let split = mv.get("split").and_then(|v| v.as_bool()).unwrap_or(false);
                let dir = match mv.get("dir").and_then(|v| v.as_str()).unwrap_or("Up") {
                    "Up" => Direction::Up,
                    "Down" => Direction::Down,
                    "Left" => Direction::Left,
                    "Right" => Direction::Right,
                    _ => Direction::Up,
                };
                actions.push(Action::Move {
                    row,
                    col,
                    dir,
                    split,
                });
            }
        }
        actions
    }
}

impl Agent for SubprocessAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "v1"
    }

    fn act(&mut self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        self.ensure_started();

        // Serialize the observation as JSON with type field.
        let obs_json = match serde_json::to_value(obs) {
            Ok(Value::Object(mut map)) => {
                map.insert("type".to_string(), Value::String("observation".to_string()));
                match serde_json::to_string(&Value::Object(map)) {
                    Ok(s) => s,
                    Err(_) => return vec![],
                }
            }
            _ => return vec![],
        };

        if !self.send_line(&obs_json) {
            eprintln!(
                "Failed to send observation to subprocess agent '{}'",
                self.name
            );
            return vec![];
        }

        match self.read_line() {
            Some(response) => Self::parse_actions(&response),
            None => {
                eprintln!("No response from subprocess agent '{}'", self.name);
                vec![]
            }
        }
    }

    fn reset(&mut self) {
        self.ensure_started();
        let _ = self.send_line(r#"{"type": "reset"}"#);
    }
}

impl Drop for SubprocessAgent {
    fn drop(&mut self) {
        if let Some(mut handle) = self.child.take() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }
    }
}
