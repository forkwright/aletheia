//! Tool group policy types: capability gating, role enforcement, and call classification.

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq as _, Serializer};
use serde::{Deserialize, Serialize};

/// Machine-checkable tool groups for role-based gating.
///
/// Each tool declares which groups it belongs to; roles declare which groups
/// they are allowed to use.  The registry filters the tool surface presented
/// to the LLM and rejects out-of-group calls at dispatch time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolGroupId {
    /// File/code reading tools (`read`, `grep`, `find`, `ls`, `view_file`, ...).
    Read,
    /// File, memory, or session-state mutation tools (`write`, `edit`, `memory_correct`, ...).
    Edit,
    /// Shell/cargo execution (`exec`, `git_checkout`, `computer_use`, ...).
    Command,
    /// MCP tool invocation and external API calls (`web_fetch`, `http_request`, ...).
    Mcp,
    /// Spawning sub-agents (`sessions_spawn`, `sessions_dispatch`, ...).
    SpawnSubtask,
    /// Planning and design tools (`plan_create`, `plan_roadmap`, ...).
    Plan,
    /// Tests, lint, fmt, and verification tools (`lint_report`, `verify_report`, ...).
    Verify,
}

impl std::str::FromStr for ToolGroupId {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
            "command" => Ok(Self::Command),
            "mcp" => Ok(Self::Mcp),
            "spawn_subtask" => Ok(Self::SpawnSubtask),
            "plan" => Ok(Self::Plan),
            "verify" => Ok(Self::Verify),
            other => Err(format!("unknown tool group: {other}")),
        }
    }
}

impl std::fmt::Display for ToolGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => f.write_str("read"),
            Self::Edit => f.write_str("edit"),
            Self::Command => f.write_str("command"),
            Self::Mcp => f.write_str("mcp"),
            Self::SpawnSubtask => f.write_str("spawn_subtask"),
            Self::Plan => f.write_str("plan"),
            Self::Verify => f.write_str("verify"),
        }
    }
}

/// Explicit role policy for tool-group gating.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ToolGroupPolicy {
    /// Every registered tool is permitted, including tools with no group metadata.
    AllowAll {
        /// Human-readable reason for granting every tool group.
        reason: String,
    },
    /// Tools are permitted when their declared groups intersect this list.
    Groups(Vec<ToolGroupId>),
    /// No grouped tools are permitted.
    #[default]
    DenyAll,
}

impl ToolGroupPolicy {
    /// Build an explicit group policy, treating an empty list as deny-all.
    #[must_use]
    pub fn groups(groups: Vec<ToolGroupId>) -> Self {
        if groups.is_empty() {
            Self::DenyAll
        } else {
            Self::Groups(groups)
        }
    }

    /// Returns true when the policy permits a tool with the given groups.
    #[must_use]
    pub fn permits(&self, tool_groups: &[ToolGroupId]) -> bool {
        match self {
            Self::AllowAll { .. } => true,
            Self::Groups(allowed) => {
                !tool_groups.is_empty() && tool_groups.iter().any(|group| allowed.contains(group))
            }
            Self::DenyAll => false,
        }
    }

    /// The concrete allowed groups, if this is a group policy.
    #[must_use]
    pub fn allowed_groups(&self) -> &[ToolGroupId] {
        match self {
            Self::Groups(groups) => groups,
            Self::AllowAll { .. } | Self::DenyAll => &[],
        }
    }

    /// Human-readable policy text for denial messages and prompts.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::AllowAll { reason } => format!("all ({reason})"),
            Self::Groups(groups) => groups
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            Self::DenyAll => "deny".to_owned(),
        }
    }
}

/// Effective capability classification for one tool call.
///
/// Tool-level groups and reversibility describe the default call shape.
/// Mixed tools can attach argument-driven rules so execution-time policy
/// checks use the selected operation rather than the broad tool surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallCapability {
    /// Tool groups required by this concrete call.
    pub groups: Vec<ToolGroupId>,
    /// Reversibility for this concrete call.
    pub reversibility: super::Reversibility,
}

impl ToolCallCapability {
    /// Build a call capability from groups and reversibility.
    #[must_use]
    pub fn new(groups: Vec<ToolGroupId>, reversibility: super::Reversibility) -> Self {
        Self {
            groups,
            reversibility,
        }
    }
}

/// Argument value mapped to an effective call capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArgumentValueCapability {
    /// Selector value from the tool input.
    pub value: String,
    /// Capability for calls carrying this selector value.
    pub capability: ToolCallCapability,
}

/// Argument-driven capability metadata for tools with mixed operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCallCapabilityRule {
    /// Classify by an argument's string value, such as `action` or `op`.
    ArgumentValue {
        /// Argument name to read from the tool input.
        argument: String,
        /// Capabilities keyed by argument value.
        values: Vec<ToolArgumentValueCapability>,
    },
    /// Classify by whether an argument is present.
    ArgumentPresence {
        /// Argument name to test in the tool input.
        argument: String,
        /// Capability when the argument is present and not null.
        present: ToolCallCapability,
        /// Capability when the argument is absent or null.
        absent: ToolCallCapability,
    },
}

impl ToolCallCapabilityRule {
    /// Build a string-value selector.
    #[must_use]
    pub fn argument_value<V, I>(argument: impl Into<String>, values: I) -> Self
    where
        V: Into<String>,
        I: IntoIterator<Item = (V, ToolCallCapability)>,
    {
        Self::ArgumentValue {
            argument: argument.into(),
            values: values
                .into_iter()
                .map(|(value, capability)| ToolArgumentValueCapability {
                    value: value.into(),
                    capability,
                })
                .collect(),
        }
    }

    /// Build a presence selector.
    #[must_use]
    pub fn argument_presence(
        argument: impl Into<String>,
        present: ToolCallCapability,
        absent: ToolCallCapability,
    ) -> Self {
        Self::ArgumentPresence {
            argument: argument.into(),
            present,
            absent,
        }
    }

    /// Classify one set of tool arguments.
    ///
    /// # Errors
    ///
    /// Returns an error when a value selector argument is missing, non-string,
    /// or has no matching classification.
    pub fn classify(
        &self,
        arguments: &serde_json::Value,
    ) -> std::result::Result<ToolCallCapability, String> {
        match self {
            Self::ArgumentValue { argument, values } => {
                let Some(value) = arguments.get(argument).and_then(serde_json::Value::as_str)
                else {
                    return Err(format!("missing or invalid operation selector: {argument}"));
                };
                values
                    .iter()
                    .find(|case| case.value == value)
                    .map(|case| case.capability.clone())
                    .ok_or_else(|| {
                        format!("unknown operation selector value for {argument}: {value}")
                    })
            }
            Self::ArgumentPresence {
                argument,
                present,
                absent,
            } => {
                if arguments
                    .get(argument)
                    .is_some_and(|value| !value.is_null())
                {
                    Ok(present.clone())
                } else {
                    Ok(absent.clone())
                }
            }
        }
    }
}

impl Serialize for ToolGroupPolicy {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::AllowAll { .. } => serializer.serialize_str("all"),
            Self::DenyAll => serializer.serialize_str("deny"),
            Self::Groups(groups) => {
                let mut seq = serializer.serialize_seq(Some(groups.len()))?;
                for group in groups {
                    seq.serialize_element(group)?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ToolGroupPolicy {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PolicyVisitor;

        impl<'de> Visitor<'de> for PolicyVisitor {
            type Value = ToolGroupPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(r#""all", "deny", or a list of tool group names"#)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "all" => Ok(ToolGroupPolicy::AllowAll {
                        reason: "explicit tool_groups = \"all\"".to_owned(),
                    }),
                    "deny" => Ok(ToolGroupPolicy::DenyAll),
                    other => Err(E::custom(format!(
                        "unknown tool group policy {other:?}; expected \"all\", \"deny\", or a list"
                    ))),
                }
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut groups = Vec::new();
                while let Some(group) = seq.next_element()? {
                    groups.push(group);
                }
                Ok(ToolGroupPolicy::groups(groups))
            }
        }

        deserializer.deserialize_any(PolicyVisitor)
    }
}
