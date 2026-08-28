//! Shared capability policy for Poiesis/report built-in tools.
//!
//! ARCHITECTURE(#7030): every report tool used to hand-write its own
//! `ToolCallCapabilityRule` and `ToolCapabilityMetadata`, and all but one
//! restated the identical rule (`out_path`/`directory` present -> `Edit` +
//! `PartiallyReversible`, absent -> `Read` + `FullyReversible`). That made a
//! change to output-write semantics or reversibility a manual sweep across
//! eight files, and it forced a known misclassification: a subprocess-backed
//! render (`render_deck_report(format="pdf")` with no `out_path`) has a real
//! effect -- a Chromium child process -- that a single-argument rule cannot
//! express alongside the file-write axis, so it read as the same
//! `Read` + `FullyReversible` as a call that touches nothing at all.
//!
//! [`ReportToolEffect`] is the one declaration a report tool now writes: the
//! output-write shape (a single caller-named file, or a caller-named
//! directory scaffold) plus an optional independent subprocess effect. Both
//! the call-capability rule and the governance metadata (owner, stability,
//! rollback text) derive from it, so the two can never drift from each
//! other or from the executor they describe.

use crate::types::{
    Reversibility, RollbackSupport, ToolCallCapability, ToolCallCapabilityRule, ToolCallCondition,
    ToolCapabilityMetadata, ToolGroupId, ToolStability,
};

/// The output-write axis a report tool classifies its call on.
pub(crate) enum ReportOutputEffect {
    /// The tool always renders in memory; a caller-supplied path argument
    /// additionally writes one rendered artifact to disk, overwriting any
    /// existing file at that path without retaining its prior contents.
    CallerFile {
        /// Argument name carrying the output path (e.g. `"out_path"`).
        argument: &'static str,
        /// Artifact description for the rollback reason text, e.g.
        /// `"the PPTX"` or `"the rendered bytes"`.
        artifact: &'static str,
    },
    /// A caller-supplied directory argument causes the tool to create
    /// directories and write multiple template files into it, overwriting
    /// same-named paths in place; without it the tool returns a base64
    /// manifest only.
    DirectoryScaffold {
        /// Argument name carrying the target directory (e.g. `"directory"`).
        argument: &'static str,
    },
}

/// An effect independent of the output-write axis: a subprocess the tool
/// spawns regardless of whether a file is written. Composes with
/// [`ReportOutputEffect`] rather than replacing it -- a call can both spawn
/// the subprocess and write to a caller path.
pub(crate) struct SubprocessEffect {
    /// Argument that selects the mode/format triggering the subprocess.
    pub argument: &'static str,
    /// Argument values that spawn the subprocess.
    pub values: &'static [&'static str],
}

/// One report tool's declared effect surface. Built once per tool in its
/// `register()` and consumed by both [`Self::capability_rule`] and
/// [`Self::capability_metadata`], so the call-capability rule and the
/// governance declaration describe the same effects by construction.
pub(crate) struct ReportToolEffect {
    /// Owning module path, e.g. `"organon::builtins::render_deck_report"`.
    pub owner: &'static str,
    /// The output-write axis.
    pub output: ReportOutputEffect,
    /// An independent subprocess effect, when the tool has one.
    pub subprocess: Option<SubprocessEffect>,
}

impl ReportToolEffect {
    /// Capability when the output-write axis is NOT active (memory-only:
    /// bytes returned, nothing on disk).
    fn memory_only_capability() -> ToolCallCapability {
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible)
    }

    /// Capability when the output-write axis IS active (a file or directory
    /// is written).
    fn write_capability() -> ToolCallCapability {
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible)
    }

    /// Capability for a call that spawns the subprocess but does not write
    /// to a caller path: more than a pure read (an external process ran)
    /// but nothing persists to roll back, so it sits between the two file
    /// axes rather than collapsing into either.
    fn subprocess_only_capability() -> ToolCallCapability {
        ToolCallCapability::new(
            vec![ToolGroupId::Read, ToolGroupId::Command],
            Reversibility::Reversible,
        )
    }

    fn output_argument(&self) -> &'static str {
        match &self.output {
            ReportOutputEffect::CallerFile { argument, .. }
            | ReportOutputEffect::DirectoryScaffold { argument } => argument,
        }
    }

    /// Derive the call-capability rule for this tool's declared effects.
    pub(crate) fn capability_rule(&self) -> ToolCallCapabilityRule {
        let output_argument = self.output_argument();
        match &self.subprocess {
            None => ToolCallCapabilityRule::argument_presence(
                output_argument,
                Self::write_capability(),
                Self::memory_only_capability(),
            ),
            Some(subprocess) => ToolCallCapabilityRule::decision(
                [
                    // The write axis dominates: a caller path always writes,
                    // regardless of whether the subprocess also runs.
                    (
                        vec![ToolCallCondition::ArgumentPresent {
                            argument: output_argument.to_owned(),
                        }],
                        Self::write_capability(),
                    ),
                    // No write, but the subprocess-triggering value was
                    // selected: the known-weak case #7030 exists to fix.
                    (
                        vec![ToolCallCondition::ArgumentValueIn {
                            argument: subprocess.argument.to_owned(),
                            values: subprocess.values.iter().map(|v| (*v).to_owned()).collect(),
                        }],
                        Self::subprocess_only_capability(),
                    ),
                ],
                Self::memory_only_capability(),
            ),
        }
    }

    /// Derive the rollback reason text for the output-write axis.
    fn rollback_reason(&self) -> String {
        match &self.output {
            ReportOutputEffect::CallerFile { argument, artifact } => format!(
                "rendering runs in memory; a caller-provided {argument} writes {artifact} to \
                 disk, overwriting any existing file without retaining its prior contents"
            ),
            ReportOutputEffect::DirectoryScaffold { argument } => format!(
                "without a {argument} argument the tool returns a base64 manifest only; with \
                 {argument} it creates directories and writes template files, overwriting \
                 same-named paths without retaining their prior contents"
            ),
        }
    }

    /// Derive the governance declaration for this tool's declared effects.
    ///
    /// Stability is always [`ToolStability::Experimental`]: every report
    /// tool lives behind the `poiesis` cargo feature gate (see
    /// `crates/organon/src/builtins/mod.rs`) and is not compiled by
    /// default.
    pub(crate) fn capability_metadata(&self) -> ToolCapabilityMetadata {
        ToolCapabilityMetadata {
            owner: self.owner.to_owned(),
            stability: ToolStability::Experimental,
            rollback: RollbackSupport::PartialSupport {
                reason: self.rollback_reason(),
            },
            ..ToolCapabilityMetadata::default()
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn classify(rule: &ToolCallCapabilityRule, args: serde_json::Value) -> ToolCallCapability {
        rule.classify(&args).expect("classification succeeds")
    }

    #[test]
    fn caller_file_rule_matches_argument_presence() {
        let effect = ReportToolEffect {
            owner: "organon::builtins::render_pptx_report",
            output: ReportOutputEffect::CallerFile {
                argument: "out_path",
                artifact: "the PPTX",
            },
            subprocess: None,
        };
        let rule = effect.capability_rule();

        let present = classify(&rule, serde_json::json!({"out_path": "/tmp/x.pptx"}));
        assert_eq!(present.groups, vec![ToolGroupId::Edit]);
        assert_eq!(present.reversibility, Reversibility::PartiallyReversible);

        let absent = classify(&rule, serde_json::json!({}));
        assert_eq!(absent.groups, vec![ToolGroupId::Read]);
        assert_eq!(absent.reversibility, Reversibility::FullyReversible);
    }

    #[test]
    fn directory_scaffold_rollback_text_names_the_argument() {
        let effect = ReportToolEffect {
            owner: "organon::builtins::scaffold_report",
            output: ReportOutputEffect::DirectoryScaffold {
                argument: "directory",
            },
            subprocess: None,
        };
        let RollbackSupport::PartialSupport { reason } = effect.capability_metadata().rollback
        else {
            panic!("expected PartialSupport");
        };
        assert!(reason.contains("directory"));
        assert!(reason.contains("base64 manifest"));
    }

    #[test]
    fn subprocess_effect_composes_with_write_axis() {
        let effect = ReportToolEffect {
            owner: "organon::builtins::render_deck_report",
            output: ReportOutputEffect::CallerFile {
                argument: "out_path",
                artifact: "the rendered HTML/PDF",
            },
            subprocess: Some(SubprocessEffect {
                argument: "format",
                values: &["pdf"],
            }),
        };
        let rule = effect.capability_rule();

        // out_path present -> write, regardless of format.
        let write = classify(
            &rule,
            serde_json::json!({"format": "pdf", "out_path": "/tmp/x.pdf"}),
        );
        assert_eq!(write.groups, vec![ToolGroupId::Edit]);
        assert_eq!(write.reversibility, Reversibility::PartiallyReversible);

        // format == pdf, no out_path -> elevated subprocess classification,
        // not the same FullyReversible read as a no-op call.
        let subprocess_only = classify(&rule, serde_json::json!({"format": "pdf"}));
        assert_eq!(
            subprocess_only.groups,
            vec![ToolGroupId::Read, ToolGroupId::Command]
        );
        assert_eq!(subprocess_only.reversibility, Reversibility::Reversible);

        // Neither axis active -> plain memory-only read.
        let plain = classify(&rule, serde_json::json!({"format": "html"}));
        assert_eq!(plain.groups, vec![ToolGroupId::Read]);
        assert_eq!(plain.reversibility, Reversibility::FullyReversible);
    }
}
