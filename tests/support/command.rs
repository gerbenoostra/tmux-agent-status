//! Parsing and validation of hook command strings.

use tmux_agent_status::state::State;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HookCommand {
    Set(String),
    Start,
    Reset,
    Finish,
    ClearPane,
    Notify { agent: String },
}

impl HookCommand {
    /// The full command as it would appear in a shipped file.
    pub fn as_str(&self) -> String {
        match self {
            HookCommand::Set(state) => format!("tmux-agent-status set {state}"),
            HookCommand::Start => "tmux-agent-status start".to_string(),
            HookCommand::Reset => "tmux-agent-status reset".to_string(),
            HookCommand::Finish => "tmux-agent-status finish".to_string(),
            HookCommand::ClearPane => "tmux-agent-status clear-pane".to_string(),
            HookCommand::Notify { agent } => format!("tmux-agent-status notify --agent {agent}"),
        }
    }

    /// Only the arguments after `tmux-agent-status `.
    pub fn arguments(&self) -> String {
        match self {
            HookCommand::Set(state) => format!("set {state}"),
            HookCommand::Start => "start".to_string(),
            HookCommand::Reset => "reset".to_string(),
            HookCommand::Finish => "finish".to_string(),
            HookCommand::ClearPane => "clear-pane".to_string(),
            HookCommand::Notify { agent } => format!("notify --agent {agent}"),
        }
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
        ["reset"] => HookCommand::Reset,
        ["finish"] => HookCommand::Finish,
        ["clear-pane"] => HookCommand::ClearPane,
        ["notify", "--agent", agent, ..] => HookCommand::Notify {
            agent: agent.to_string(),
        },
        _ => panic!("{source}: unrecognised tmux-agent-status command: {command}"),
    }
}
