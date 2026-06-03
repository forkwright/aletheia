//! Signal `!`-command parser and dispatcher.
//!
//! An inbound message whose text starts with `!` is parsed as a structured command
//! (name + args) instead of a plain conversational turn. Non-`!` messages are
//! unaffected. Unknown `!` commands return a helpful error listing available names.

use std::fmt::Write as _;

/// A fully-parsed `!`-command extracted from an inbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// `!help` — list all available commands.
    Help,
    /// `!status` — lifecycle + session info for the routed nous agent.
    Status,
    /// `!agents` — list all running agents.
    Agents,
    /// `!whoami` — report which agent will receive this conversation.
    WhoAmI,
    /// `!new [session_name]` — start a fresh session (optional label).
    New {
        /// Optional human-readable label for the new session.
        label: Option<String>,
    },
    /// `!end` — close the current session for this conversation thread.
    End,
    /// `!sessions` — list sessions tracked by the routed nous agent.
    Sessions,
    /// `!ping` — liveness check (no agent turn, just a round-trip ack).
    Ping,
    /// `!channels` — report registered channel providers and their health.
    Channels,
    /// `!uptime` — agent uptime and panic-boundary count.
    Uptime,
    /// `!model` — show the model currently configured for the routed agent.
    Model,
    /// `!info [agent_id]` — detail view of a specific or current agent.
    Info {
        /// Agent identifier to inspect; `None` means the current routed agent.
        agent_id: Option<String>,
    },
    /// Unknown command: carries the unrecognised name for the error reply.
    Unknown {
        /// The unrecognised command name (without `!`).
        name: String,
    },
}

impl Command {
    /// Return the canonical name of this command (without `!`).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Help => "help",
            Self::Status => "status",
            Self::Agents => "agents",
            Self::WhoAmI => "whoami",
            Self::New { .. } => "new",
            Self::End => "end",
            Self::Sessions => "sessions",
            Self::Ping => "ping",
            Self::Channels => "channels",
            Self::Uptime => "uptime",
            Self::Model => "model",
            Self::Info { .. } => "info",
            Self::Unknown { name } => name.as_str(),
        }
    }
}

/// All commands known to the dispatcher, in display order.
const KNOWN_COMMANDS: &[(&str, &str)] = &[
    ("!help", "list all available commands"),
    ("!status", "lifecycle and session info for this agent"),
    ("!agents", "list all running agents"),
    ("!whoami", "show which agent handles this conversation"),
    ("!new [label]", "start a fresh session (optional label ignored by agent)"),
    ("!end", "close the current session"),
    ("!sessions", "count sessions tracked by this agent"),
    ("!ping", "round-trip liveness check"),
    ("!channels", "list channel providers and health"),
    ("!uptime", "agent uptime and panic-boundary count"),
    ("!model", "show the LLM model configured for this agent"),
    ("!info [agent_id]", "detail view for an agent (default: current)"),
];

/// Parse an inbound message text into a `Command`.
///
/// Returns `None` when the text does not start with `!`, signalling that the
/// message should be delivered as a plain turn.
#[must_use]
pub fn parse(text: &str) -> Option<Command> {
    let text = text.trim();
    if !text.starts_with('!') {
        return None;
    }

    // WHY: starts_with('!') guarantees at least one byte; '!' is ASCII (1 byte), safe to skip.
    #[expect(
        clippy::string_slice,
        reason = "safe: '!' is single-byte ASCII, starts_with check above guarantees index valid"
    )]
    let without_bang = text[1..].trim();
    let (name, rest) = without_bang
        .split_once(char::is_whitespace)
        .map_or((without_bang, ""), |(n, r)| (n, r.trim()));

    let cmd = match name.to_ascii_lowercase().as_str() {
        "help" | "h" | "?" => Command::Help,
        "status" | "s" => Command::Status,
        "agents" => Command::Agents,
        "whoami" | "who" => Command::WhoAmI,
        "new" => Command::New {
            label: if rest.is_empty() {
                None
            } else {
                Some(rest.to_owned())
            },
        },
        "end" | "quit" | "q" => Command::End,
        "sessions" | "sess" => Command::Sessions,
        "ping" => Command::Ping,
        "channels" | "ch" => Command::Channels,
        "uptime" => Command::Uptime,
        "model" => Command::Model,
        "info" => Command::Info {
            agent_id: if rest.is_empty() {
                None
            } else {
                Some(rest.to_owned())
            },
        },
        unknown => Command::Unknown {
            name: unknown.to_owned(),
        },
    };
    Some(cmd)
}

/// Agent-level status snapshot passed by the binary into the command context.
#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    /// Agent identifier.
    pub id: String,
    /// Lifecycle state as a display string (e.g., "idle", "active").
    pub lifecycle: String,
    /// Number of in-memory sessions tracked.
    pub session_count: usize,
    /// Currently active session key, if any.
    pub active_session: Option<String>,
    /// Panic-boundary hit count since last restart.
    pub panic_count: u32,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Configured LLM model name.
    pub model: String,
}

/// Channel health summary passed by the binary into the command context.
#[derive(Debug, Clone)]
pub struct ChannelSnapshot {
    /// Channel identifier (e.g., "signal").
    pub id: String,
    /// Whether the last probe succeeded.
    pub healthy: bool,
    /// Round-trip latency in milliseconds from the last probe, if measured.
    pub latency_ms: Option<u64>,
}

/// Everything the command dispatcher needs to formulate a response.
///
/// The binary fills this from `NousManager` + `ChannelRegistry` data before
/// invoking [`execute`]. All fields are cheap clones of snapshot data.
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// The nous agent that would normally handle this conversation.
    pub current_nous_id: String,
    /// Session key identifying this conversation thread.
    pub session_key: String,
    /// Status snapshot for the current agent (if available).
    pub current_agent: Option<AgentSnapshot>,
    /// All running agent snapshots.
    pub all_agents: Vec<AgentSnapshot>,
    /// Channel health snapshots (empty when probe was not run).
    pub channels: Vec<ChannelSnapshot>,
}

/// Execute a parsed command against the given context and return a reply string.
///
/// The returned string is ready to be sent back through the channel. Every
/// command variant is handled: unknown commands get a helpful error.
#[must_use]
pub fn execute(cmd: &Command, ctx: &CommandContext) -> String {
    match cmd {
        Command::Help => cmd_help(),
        Command::Status => cmd_status(ctx),
        Command::Agents => cmd_agents(ctx),
        Command::WhoAmI => cmd_whoami(ctx),
        Command::New { label } => cmd_new(ctx, label.as_deref()),
        Command::End => cmd_end(ctx),
        Command::Sessions => cmd_sessions(ctx),
        Command::Ping => cmd_ping(ctx),
        Command::Channels => cmd_channels(ctx),
        Command::Uptime => cmd_uptime(ctx),
        Command::Model => cmd_model(ctx),
        Command::Info { agent_id } => cmd_info(ctx, agent_id.as_deref()),
        Command::Unknown { name } => cmd_unknown(name),
    }
}

fn cmd_help() -> String {
    let mut out = "Available commands:\n".to_owned();
    for (cmd, desc) in KNOWN_COMMANDS {
        let _ = writeln!(out, "  {cmd} — {desc}");
    }
    out.trim_end().to_owned()
}

fn cmd_status(ctx: &CommandContext) -> String {
    match &ctx.current_agent {
        None => format!(
            "Agent '{}' status unavailable (not responding).",
            ctx.current_nous_id
        ),
        Some(a) => {
            let session_info = a
                .active_session
                .as_deref()
                .map_or("none".to_owned(), |s| format!("'{s}'"));
            format!(
                "Agent: {id}\nLifecycle: {lc}\nSessions: {sc} (active: {si})\nPanics: {pc}\nModel: {m}\nUptime: {up}",
                id = a.id,
                lc = a.lifecycle,
                sc = a.session_count,
                si = session_info,
                pc = a.panic_count,
                m = a.model,
                up = format_uptime(a.uptime_secs),
            )
        }
    }
}

fn cmd_agents(ctx: &CommandContext) -> String {
    if ctx.all_agents.is_empty() {
        return "No agents running.".to_owned();
    }
    let mut out = format!("{} agent(s) running:\n", ctx.all_agents.len());
    for a in &ctx.all_agents {
        let marker = if a.id == ctx.current_nous_id { " *" } else { "" };
        let _ = writeln!(out, "  {}{} ({})", a.id, marker, a.lifecycle);
    }
    out.push_str("(* = current)");
    out
}

fn cmd_whoami(ctx: &CommandContext) -> String {
    format!(
        "This conversation routes to agent '{}'.\nSession key: {}",
        ctx.current_nous_id, ctx.session_key
    )
}

fn cmd_new(ctx: &CommandContext, label: Option<&str>) -> String {
    // NOTE: Session reset is handled by the session store: a new session key
    // is derived from the label (or a new turn starts a new session automatically
    // once the current session key is retired). The command acknowledges intent;
    // the next plain turn will open a fresh session under this agent.
    let note = label.map_or_else(String::new, |l| format!(" (label: '{l}')"));
    format!(
        "Session reset requested{note}.\nSend your next message to start a new conversation with agent '{}'.",
        ctx.current_nous_id
    )
}

fn cmd_end(ctx: &CommandContext) -> String {
    format!(
        "Session '{}' with agent '{}' ended. Send any message to start a new conversation.",
        ctx.session_key, ctx.current_nous_id
    )
}

fn cmd_sessions(ctx: &CommandContext) -> String {
    match &ctx.current_agent {
        None => format!(
            "Agent '{}' status unavailable.",
            ctx.current_nous_id
        ),
        Some(a) => {
            let active = a
                .active_session
                .as_deref()
                .map_or("none".to_owned(), |s| format!("'{s}'"));
            format!(
                "Agent '{}': {} session(s) in memory, active: {}.",
                a.id, a.session_count, active
            )
        }
    }
}

fn cmd_ping(ctx: &CommandContext) -> String {
    match &ctx.current_agent {
        None => format!("Agent '{}' is not responding.", ctx.current_nous_id),
        Some(a) => format!("Pong. Agent '{}' is alive ({}).", a.id, a.lifecycle),
    }
}

fn cmd_channels(ctx: &CommandContext) -> String {
    if ctx.channels.is_empty() {
        return "Channel health data not available (probe not run).".to_owned();
    }
    let mut out = format!("{} channel(s):\n", ctx.channels.len());
    for ch in &ctx.channels {
        let status = if ch.healthy { "ok" } else { "unhealthy" };
        let latency = ch
            .latency_ms
            .map_or_else(String::new, |ms| format!(", {ms}ms"));
        let _ = writeln!(out, "  {} — {}{}", ch.id, status, latency);
    }
    out.trim_end().to_owned()
}

fn cmd_uptime(ctx: &CommandContext) -> String {
    match &ctx.current_agent {
        None => format!("Agent '{}' status unavailable.", ctx.current_nous_id),
        Some(a) => format!(
            "Agent '{}': uptime {}, panics: {}.",
            a.id,
            format_uptime(a.uptime_secs),
            a.panic_count,
        ),
    }
}

fn cmd_model(ctx: &CommandContext) -> String {
    match &ctx.current_agent {
        None => format!("Agent '{}' status unavailable.", ctx.current_nous_id),
        Some(a) => format!("Agent '{}' model: {}.", a.id, a.model),
    }
}

fn cmd_info(ctx: &CommandContext, agent_id: Option<&str>) -> String {
    let target_id = agent_id.unwrap_or(&ctx.current_nous_id);
    let agent = ctx.all_agents.iter().find(|a| a.id == target_id);
    match agent {
        None => format!("Agent '{target_id}' not found."),
        Some(a) => {
            let active = a
                .active_session
                .as_deref()
                .map_or("none".to_owned(), |s| format!("'{s}'"));
            format!(
                "Agent: {id}\nLifecycle: {lc}\nModel: {m}\nSessions: {sc} (active: {ai})\nPanics: {pc}\nUptime: {up}",
                id = a.id,
                lc = a.lifecycle,
                m = a.model,
                sc = a.session_count,
                ai = active,
                pc = a.panic_count,
                up = format_uptime(a.uptime_secs),
            )
        }
    }
}

fn cmd_unknown(name: &str) -> String {
    format!(
        "Unknown command '!{name}'. Type !help for a list of available commands."
    )
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_plain_message_returns_none() {
        assert!(parse("hello world").is_none());
        assert!(parse("how are you?").is_none());
        assert!(parse("").is_none());
        assert!(parse("   ").is_none());
    }

    #[test]
    fn parse_help_variants() {
        assert_eq!(parse("!help"), Some(Command::Help));
        assert_eq!(parse("!h"), Some(Command::Help));
        assert_eq!(parse("!?"), Some(Command::Help));
        assert_eq!(parse("  !help  "), Some(Command::Help));
    }

    #[test]
    fn parse_status() {
        assert_eq!(parse("!status"), Some(Command::Status));
        assert_eq!(parse("!s"), Some(Command::Status));
    }

    #[test]
    fn parse_agents() {
        assert_eq!(parse("!agents"), Some(Command::Agents));
    }

    #[test]
    fn parse_whoami_variants() {
        assert_eq!(parse("!whoami"), Some(Command::WhoAmI));
        assert_eq!(parse("!who"), Some(Command::WhoAmI));
    }

    #[test]
    fn parse_new_no_label() {
        assert_eq!(parse("!new"), Some(Command::New { label: None }));
    }

    #[test]
    fn parse_new_with_label() {
        assert_eq!(
            parse("!new my-session"),
            Some(Command::New {
                label: Some("my-session".to_owned())
            })
        );
        assert_eq!(
            parse("!new   work stuff  "),
            Some(Command::New {
                label: Some("work stuff".to_owned())
            })
        );
    }

    #[test]
    fn parse_end_variants() {
        assert_eq!(parse("!end"), Some(Command::End));
        assert_eq!(parse("!quit"), Some(Command::End));
        assert_eq!(parse("!q"), Some(Command::End));
    }

    #[test]
    fn parse_sessions() {
        assert_eq!(parse("!sessions"), Some(Command::Sessions));
        assert_eq!(parse("!sess"), Some(Command::Sessions));
    }

    #[test]
    fn parse_ping() {
        assert_eq!(parse("!ping"), Some(Command::Ping));
    }

    #[test]
    fn parse_channels() {
        assert_eq!(parse("!channels"), Some(Command::Channels));
        assert_eq!(parse("!ch"), Some(Command::Channels));
    }

    #[test]
    fn parse_uptime() {
        assert_eq!(parse("!uptime"), Some(Command::Uptime));
    }

    #[test]
    fn parse_model() {
        assert_eq!(parse("!model"), Some(Command::Model));
    }

    #[test]
    fn parse_info_no_arg() {
        assert_eq!(parse("!info"), Some(Command::Info { agent_id: None }));
    }

    #[test]
    fn parse_info_with_agent() {
        assert_eq!(
            parse("!info syn"),
            Some(Command::Info {
                agent_id: Some("syn".to_owned())
            })
        );
    }

    #[test]
    fn parse_unknown_command() {
        assert_eq!(
            parse("!frobnicate"),
            Some(Command::Unknown {
                name: "frobnicate".to_owned()
            })
        );
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(parse("!HELP"), Some(Command::Help));
        assert_eq!(parse("!Status"), Some(Command::Status));
        assert_eq!(parse("!AGENTS"), Some(Command::Agents));
    }

    // Dispatch tests
    // -----------------------------------------------------------------------

    fn make_context() -> CommandContext {
        CommandContext {
            current_nous_id: "syn".to_owned(),
            session_key: "signal:+15550100".to_owned(),
            current_agent: Some(AgentSnapshot {
                id: "syn".to_owned(),
                lifecycle: "idle".to_owned(),
                session_count: 3,
                active_session: Some("signal:+15550100".to_owned()),
                panic_count: 0,
                uptime_secs: 3661,
                model: "claude-sonnet-4-6".to_owned(),
            }),
            all_agents: vec![AgentSnapshot {
                id: "syn".to_owned(),
                lifecycle: "idle".to_owned(),
                session_count: 3,
                active_session: Some("signal:+15550100".to_owned()),
                panic_count: 0,
                uptime_secs: 3661,
                model: "claude-sonnet-4-6".to_owned(),
            }],
            channels: vec![ChannelSnapshot {
                id: "signal".to_owned(),
                healthy: true,
                latency_ms: Some(12),
            }],
        }
    }

    #[test]
    fn help_lists_all_known_commands() {
        let ctx = make_context();
        let reply = execute(&Command::Help, &ctx);
        // Every known command name should appear in the help output.
        for (cmd, _) in KNOWN_COMMANDS {
            assert!(
                reply.contains(cmd),
                "help output missing '{cmd}': {reply}"
            );
        }
    }

    #[test]
    fn status_includes_agent_id_and_lifecycle() {
        let ctx = make_context();
        let reply = execute(&Command::Status, &ctx);
        assert!(reply.contains("syn"), "status missing agent id: {reply}");
        assert!(reply.contains("idle"), "status missing lifecycle: {reply}");
        assert!(
            reply.contains("claude-sonnet-4-6"),
            "status missing model: {reply}"
        );
    }

    #[test]
    fn status_unavailable_when_no_snapshot() {
        let mut ctx = make_context();
        ctx.current_agent = None;
        let reply = execute(&Command::Status, &ctx);
        assert!(reply.contains("unavailable"), "expected unavailable: {reply}");
    }

    #[test]
    fn agents_lists_running_agents() {
        let ctx = make_context();
        let reply = execute(&Command::Agents, &ctx);
        assert!(reply.contains("syn"), "agents missing syn: {reply}");
        assert!(reply.contains("1 agent"), "agents missing count: {reply}");
    }

    #[test]
    fn agents_empty_when_no_agents() {
        let mut ctx = make_context();
        ctx.all_agents.clear();
        let reply = execute(&Command::Agents, &ctx);
        assert!(reply.contains("No agents"), "{reply}");
    }

    #[test]
    fn whoami_includes_nous_id_and_session_key() {
        let ctx = make_context();
        let reply = execute(&Command::WhoAmI, &ctx);
        assert!(reply.contains("syn"), "{reply}");
        assert!(reply.contains("signal:+15550100"), "{reply}");
    }

    #[test]
    fn ping_includes_alive_when_agent_present() {
        let ctx = make_context();
        let reply = execute(&Command::Ping, &ctx);
        assert!(reply.contains("alive"), "{reply}");
        assert!(reply.contains("syn"), "{reply}");
    }

    #[test]
    fn ping_not_responding_when_no_snapshot() {
        let mut ctx = make_context();
        ctx.current_agent = None;
        let reply = execute(&Command::Ping, &ctx);
        assert!(reply.contains("not responding"), "{reply}");
    }

    #[test]
    fn channels_lists_healthy_channel() {
        let ctx = make_context();
        let reply = execute(&Command::Channels, &ctx);
        assert!(reply.contains("signal"), "{reply}");
        assert!(reply.contains("ok"), "{reply}");
        assert!(reply.contains("12ms"), "{reply}");
    }

    #[test]
    fn channels_empty_when_no_probes() {
        let mut ctx = make_context();
        ctx.channels.clear();
        let reply = execute(&Command::Channels, &ctx);
        assert!(reply.contains("not available"), "{reply}");
    }

    #[test]
    fn unknown_command_suggests_help() {
        let ctx = make_context();
        let reply = execute(&Command::Unknown { name: "frobnik".to_owned() }, &ctx);
        assert!(reply.contains("frobnik"), "{reply}");
        assert!(reply.contains("!help"), "{reply}");
    }

    #[test]
    fn info_finds_current_agent() {
        let ctx = make_context();
        let reply = execute(&Command::Info { agent_id: None }, &ctx);
        assert!(reply.contains("syn"), "{reply}");
        assert!(reply.contains("claude-sonnet-4-6"), "{reply}");
    }

    #[test]
    fn info_unknown_agent_reports_not_found() {
        let ctx = make_context();
        let reply = execute(&Command::Info { agent_id: Some("nonexistent".to_owned()) }, &ctx);
        assert!(reply.contains("not found"), "{reply}");
    }

    #[test]
    fn format_uptime_seconds_only() {
        assert_eq!(format_uptime(45), "45s");
    }

    #[test]
    fn format_uptime_minutes_and_seconds() {
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(3661), "1h 1m 1s");
    }

    #[test]
    fn command_name_matches_enum_variant() {
        assert_eq!(Command::Help.name(), "help");
        assert_eq!(Command::Status.name(), "status");
        assert_eq!(Command::Agents.name(), "agents");
        assert_eq!(Command::WhoAmI.name(), "whoami");
        assert_eq!(Command::New { label: None }.name(), "new");
        assert_eq!(Command::End.name(), "end");
        assert_eq!(Command::Sessions.name(), "sessions");
        assert_eq!(Command::Ping.name(), "ping");
        assert_eq!(Command::Channels.name(), "channels");
        assert_eq!(Command::Uptime.name(), "uptime");
        assert_eq!(Command::Model.name(), "model");
        assert_eq!(Command::Info { agent_id: None }.name(), "info");
        assert_eq!(Command::Unknown { name: "xyz".to_owned() }.name(), "xyz");
    }
}
