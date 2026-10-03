//! Parsing and validation of hook command strings.

use tmux_agent_status::state::State;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HookCommand {
    Set(String),
    Start,
    /// `reset`, or `reset --agent <name> --stdin` for a host whose session
    /// payload scopes the reset.
    Reset {
        agent: Option<String>,
    },
    /// `finish`, or `finish --agent <name> --stdin`, same idea as `Reset`.
    Finish {
        agent: Option<String>,
    },
    ClearPane,
    Notify {
        agent: String,
        stdin: bool,
    },
}

impl HookCommand {
    /// The full command as it would appear in a shipped file.
    pub fn as_str(&self) -> String {
        format!("tmux-agent-status {}", self.arguments())
    }

    /// Only the arguments after `tmux-agent-status `.
    pub fn arguments(&self) -> String {
        match self {
            HookCommand::Set(state) => format!("set {state}"),
            HookCommand::Start => "start".to_string(),
            HookCommand::Reset { agent } => session_command("reset", agent),
            HookCommand::Finish { agent } => session_command("finish", agent),
            HookCommand::ClearPane => "clear-pane".to_string(),
            HookCommand::Notify { agent, stdin } => {
                let mut command = format!("notify --agent {agent}");
                if *stdin {
                    command.push_str(" --stdin");
                }
                command
            }
        }
    }
}

fn session_command(subcommand: &str, agent: &Option<String>) -> String {
    match agent {
        Some(agent) => format!("{subcommand} --agent {agent} --stdin"),
        None => subcommand.to_string(),
    }
}

/// Parse a command string from a shipped agent file or docs page.
///
/// The command is expected to start with `tmux-agent-status `. Shell
/// redirections, wrappers, and a trailing `--json` flag are stripped.
pub fn parse_command(source: &str, command: &str) -> HookCommand {
    let args = command
        .strip_prefix("tmux-agent-status ")
        .unwrap_or_else(|| {
            panic!("{source}: hook command does not invoke `tmux-agent-status`: {command}")
        });
    let head = args
        .split(|c: char| {
            c.is_whitespace() || c == '>' || c == '<' || c == '|' || c == ';' || c == '&'
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let mut head = head;
    if head.last() == Some(&"--json") {
        head.pop();
    }

    match head.as_slice() {
        ["set", state] => {
            assert!(
                State::ALL.iter().any(|s| s.name() == *state),
                "{source}: `set` has unknown state `{state}`: {command}"
            );
            HookCommand::Set(state.to_string())
        }
        ["start"] => HookCommand::Start,
        ["reset"] => HookCommand::Reset { agent: None },
        ["reset", "--agent", agent, "--stdin"] => HookCommand::Reset {
            agent: Some(agent.to_string()),
        },
        ["finish"] => HookCommand::Finish { agent: None },
        ["finish", "--agent", agent, "--stdin"] => HookCommand::Finish {
            agent: Some(agent.to_string()),
        },
        ["clear-pane"] => HookCommand::ClearPane,
        ["notify", "--agent", agent] => HookCommand::Notify {
            agent: agent.to_string(),
            stdin: false,
        },
        ["notify", "--agent", agent, "--stdin"] => HookCommand::Notify {
            agent: agent.to_string(),
            stdin: true,
        },
        _ => panic!("{source}: unrecognised tmux-agent-status command: {command}"),
    }
}
