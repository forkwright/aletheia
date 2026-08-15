#![deny(missing_docs)]
//! Parse Slack-style request text into a structured intake request.
//!
//! Keyword-based classification (no LLM call) for v1.  Reuses keyword patterns
//! from [`aletheia_lexica`] where applicable.

use aletheia_lexica::keywords::{
    INTAKE_ANALYSIS_KEYWORDS as ANALYSIS_KEYWORDS, INTAKE_DASHBOARD_KEYWORDS as DASHBOARD_KEYWORDS,
    INTAKE_REPORT_KEYWORDS as REPORT_KEYWORDS,
};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Classification of an intake request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RequestKind {
    /// Research or analytical task.
    Analysis,
    /// Written report or narrative document.
    Report,
    /// Dashboard or visual panel.
    Dashboard,
    /// Could not be classified.
    Unclassified,
}

/// A parsed intake request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct IntakeRequest {
    /// Classified kind of the request.
    pub kind: RequestKind,
    /// URL-safe slug derived from the description.
    pub slug: String,
    /// Normalised description text.
    pub description: String,
    /// Extracted requirement bullets (empty if none found).
    pub requirements: Vec<String>,
}

/// Typed B-009 front-door brief.
///
/// A `Brief` is the only intentionally free-form authoring surface before
/// downstream scaffold/render stages constrain layout, theme, and fact wiring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Brief {
    /// Delivery metadata and the single allowed theme choice.
    pub meta: BriefMeta,
    /// The one thought the audience should retain.
    pub walk_away: WalkAway,
    /// Audience roles, concerns, and desired depth.
    pub audience: Vec<Audience>,
    /// Speaker voice constraints and examples.
    pub voice: Voice,
    /// Narrative arc used by scaffold to seed deck/document/workbook structure.
    pub arc: Arc,
    /// Source-backed claims that become factbase entries.
    pub receipts: Vec<Receipt>,
    /// Visual density and aesthetic direction.
    pub aesthetic: Aesthetic,
    /// Known failure modes the authoring loop should avoid repeating.
    pub prior_failures: PriorFailures,
}

impl Brief {
    /// Return a validation report for the load-bearing B-009 fields.
    #[must_use]
    pub fn validate(&self) -> BriefValidationReport {
        let mut errors = Vec::new();

        validate_meta(&self.meta, &mut errors);
        push_required(&mut errors, "walk_away.thought", &self.walk_away.thought);
        validate_audience(&self.audience, &mut errors);
        validate_voice(&self.voice, &mut errors);
        validate_arc(&self.arc, &mut errors);
        validate_receipts(&self.receipts, &mut errors);
        validate_aesthetic(&self.aesthetic, &mut errors);
        validate_prior_failures(&self.prior_failures, &mut errors);

        BriefValidationReport {
            valid: errors.is_empty(),
            errors,
        }
    }

    /// The only authoring axes an agent may choose after intake.
    #[must_use]
    pub fn authoring_axes() -> [AuthoringAxis; 4] {
        [
            AuthoringAxis::ThemeName,
            AuthoringAxis::ComponentChoices,
            AuthoringAxis::Content,
            AuthoringAxis::FactSources,
        ]
    }
}

/// Delivery metadata from `[meta]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BriefMeta {
    /// Deliverable title.
    pub title: String,
    /// Client or audience owner.
    pub client: String,
    /// Named theme identifier; raw color/font choices are not part of intake.
    pub theme: String,
    /// Optional delivery date as authored in the brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Whether the deliverable is confidential.
    #[serde(default)]
    pub confidential: bool,
}

/// Walk-away thought from `[walk_away]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WalkAway {
    /// The one sentence the room can repeat.
    pub thought: String,
}

/// Audience entry from `[[audience]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Audience {
    /// Audience role, such as "CTO".
    pub role: String,
    /// Topics this role cares about.
    pub cares_about: Vec<String>,
    /// Desired content depth for this role.
    pub depth: AudienceDepth,
}

/// Audience depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AudienceDepth {
    /// Deep technical or analytical detail.
    Deep,
    /// Working-level detail.
    Working,
    /// Overview-level detail.
    Overview,
}

/// Speaker voice constraints from `[voice]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Voice {
    /// Five to ten exemplar lines of the speaker's real text.
    pub exemplars: Vec<String>,
    /// Register label used by downstream lint baselines.
    pub register: String,
    /// Forbidden phrases, punctuation habits, or style patterns.
    pub forbid: Vec<String>,
}

/// Narrative arc from `[arc]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Arc {
    /// Opener hook or question, not the thesis.
    pub opener: String,
    /// Ordered movement names.
    pub movements: Vec<String>,
    /// Closing thought, expected to land the walk-away.
    pub closer: String,
    /// One emotional beat per movement.
    pub emotional_beat: Vec<String>,
}

/// Source-backed claim from `[[receipts]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Receipt {
    /// Stable receipt identifier.
    pub id: String,
    /// Human-readable claim.
    pub claim: String,
    /// Typed receipt value.
    pub value: ReceiptValue,
    /// Unit for the value.
    pub unit: String,
    /// Source descriptor.
    pub source: ReceiptSource,
    /// Optional movement tag used by scaffold when pre-binding facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<String>,
}

/// Receipt value as authored at intake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ReceiptValue {
    /// Integral value.
    Integer(i64),
    /// Floating point value.
    Float(f64),
    /// Textual value for non-numeric receipts.
    Text(String),
}

impl ReceiptValue {
    fn is_empty_text(&self) -> bool {
        matches!(self, Self::Text(value) if value.trim().is_empty())
    }
}

/// Source locator for a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReceiptSource {
    /// Source kind, such as `file`, `url`, or `manual`.
    pub kind: String,
    /// Optional local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Optional JSON pointer, selector, or query locator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// Optional URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional human-readable source detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ReceiptSource {
    fn has_locator(&self) -> bool {
        [&self.path, &self.locator, &self.url, &self.detail]
            .into_iter()
            .any(|value| value.as_deref().is_some_and(|s| !s.trim().is_empty()))
    }
}

/// Aesthetic direction from `[aesthetic]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Aesthetic {
    /// Aesthetic anchor phrase.
    pub anchor: String,
    /// Aesthetics to avoid.
    pub avoid: Vec<String>,
    /// Desired density.
    pub density: Density,
}

/// Requested content density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Density {
    /// Sparse density.
    Sparse,
    /// Balanced density.
    Balanced,
    /// Dense information layout.
    Dense,
}

/// Prior failure notes from `[prior_failures]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PriorFailures {
    /// Notes describing prior deliverable failures.
    pub notes: Vec<String>,
}

/// The B-009 constrained authoring axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthoringAxis {
    /// Select one named theme.
    ThemeName,
    /// Choose components from the registry menu.
    ComponentChoices,
    /// Fill content slots.
    Content,
    /// Bind claims to source-backed receipts.
    FactSources,
}

/// One validation error for a typed brief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BriefValidationError {
    /// Field path in dotted/indexed notation.
    pub path: String,
    /// Human-readable correction.
    pub message: String,
}

/// Validation report for a typed brief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BriefValidationReport {
    /// True when the brief satisfies the B-009 load-bearing fields.
    pub valid: bool,
    /// Validation errors, empty when `valid` is true.
    pub errors: Vec<BriefValidationError>,
}

/// Errors from intake parsing.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// The intake text could not be parsed.
    #[snafu(display("intake parse failed: {message}"))]
    ParseIntake {
        /// Human-readable reason.
        message: String,
    },
    /// The typed brief is missing one or more load-bearing fields.
    #[snafu(display("brief validation failed: {}", format_brief_errors(errors)))]
    InvalidBrief {
        /// Validation errors.
        errors: Vec<BriefValidationError>,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

// ── Classification ────────────────────────────────────────────────────────────

/// Parse free-form intake text into a structured [`IntakeRequest`].
///
/// Classification is keyword-based and case-insensitive.  The first matching
/// category wins in the order: Analysis, Report, Dashboard.  If no keyword
/// matches the request is [`RequestKind::Unclassified`].
///
/// # Errors
///
/// Returns [`Error::ParseIntake`] when the input is empty or cannot be
/// normalised.
pub fn parse_intake(text: &str) -> Result<IntakeRequest> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::ParseIntake {
            message: "intake text is empty".to_owned(),
        });
    }

    let normalised = trimmed.to_lowercase();
    let kind = classify(&normalised);
    let description = trimmed.to_owned();
    let slug = slugify(&description);
    let requirements = extract_requirements(&description);

    Ok(IntakeRequest {
        kind,
        slug,
        description,
        requirements,
    })
}

/// Require a typed brief to satisfy the B-009 load-bearing field contract.
///
/// # Errors
///
/// Returns [`Error::InvalidBrief`] when required brief fields are missing.
pub fn validate_brief(brief: &Brief) -> Result<()> {
    let report = brief.validate();
    if report.valid {
        Ok(())
    } else {
        Err(Error::InvalidBrief {
            errors: report.errors,
        })
    }
}

fn push_required(errors: &mut Vec<BriefValidationError>, path: &str, value: &str) {
    if value.trim().is_empty() {
        push_error(errors, path, "must not be empty");
    }
}

fn validate_meta(meta: &BriefMeta, errors: &mut Vec<BriefValidationError>) {
    push_required(errors, "meta.title", &meta.title);
    push_required(errors, "meta.client", &meta.client);
    push_required(errors, "meta.theme", &meta.theme);
}

fn validate_audience(audience: &[Audience], errors: &mut Vec<BriefValidationError>) {
    if audience.is_empty() {
        push_error(errors, "audience", "at least one audience role is required");
    }
    for (index, entry) in audience.iter().enumerate() {
        push_required(errors, &format!("audience[{index}].role"), &entry.role);
        if entry.cares_about.is_empty() {
            push_error(
                errors,
                &format!("audience[{index}].cares_about"),
                "at least one concern is required",
            );
        }
        for (care_index, care) in entry.cares_about.iter().enumerate() {
            push_required(
                errors,
                &format!("audience[{index}].cares_about[{care_index}]"),
                care,
            );
        }
    }
}

fn validate_voice(voice: &Voice, errors: &mut Vec<BriefValidationError>) {
    if !(5..=10).contains(&voice.exemplars.len()) {
        push_error(
            errors,
            "voice.exemplars",
            "must contain 5 to 10 exemplar lines",
        );
    }
    for (index, exemplar) in voice.exemplars.iter().enumerate() {
        push_required(errors, &format!("voice.exemplars[{index}]"), exemplar);
    }
    push_required(errors, "voice.register", &voice.register);
    if voice.forbid.is_empty() {
        push_error(
            errors,
            "voice.forbid",
            "at least one forbidden voice pattern is required",
        );
    }
    for (index, forbidden) in voice.forbid.iter().enumerate() {
        push_required(errors, &format!("voice.forbid[{index}]"), forbidden);
    }
}

fn validate_arc(arc: &Arc, errors: &mut Vec<BriefValidationError>) {
    push_required(errors, "arc.opener", &arc.opener);
    if arc.movements.is_empty() {
        push_error(errors, "arc.movements", "at least one movement is required");
    }
    for (index, movement) in arc.movements.iter().enumerate() {
        push_required(errors, &format!("arc.movements[{index}]"), movement);
    }
    push_required(errors, "arc.closer", &arc.closer);
    if arc.emotional_beat.len() != arc.movements.len() {
        push_error(
            errors,
            "arc.emotional_beat",
            "must contain one beat per movement",
        );
    }
    for (index, beat) in arc.emotional_beat.iter().enumerate() {
        push_required(errors, &format!("arc.emotional_beat[{index}]"), beat);
    }
}

fn validate_receipts(receipts: &[Receipt], errors: &mut Vec<BriefValidationError>) {
    if receipts.is_empty() {
        push_error(
            errors,
            "receipts",
            "at least one source-backed receipt is required",
        );
    }
    for (index, receipt) in receipts.iter().enumerate() {
        push_required(errors, &format!("receipts[{index}].id"), &receipt.id);
        push_required(errors, &format!("receipts[{index}].claim"), &receipt.claim);
        push_required(errors, &format!("receipts[{index}].unit"), &receipt.unit);
        if receipt.value.is_empty_text() {
            push_error(
                errors,
                &format!("receipts[{index}].value"),
                "text receipt values must not be empty",
            );
        }
        push_required(
            errors,
            &format!("receipts[{index}].source.kind"),
            &receipt.source.kind,
        );
        if !receipt.source.has_locator() {
            push_error(
                errors,
                &format!("receipts[{index}].source"),
                "source must include path, url, locator, or detail",
            );
        }
    }
}

fn validate_aesthetic(aesthetic: &Aesthetic, errors: &mut Vec<BriefValidationError>) {
    push_required(errors, "aesthetic.anchor", &aesthetic.anchor);
    if aesthetic.avoid.is_empty() {
        push_error(
            errors,
            "aesthetic.avoid",
            "at least one avoided aesthetic is required",
        );
    }
    for (index, avoid) in aesthetic.avoid.iter().enumerate() {
        push_required(errors, &format!("aesthetic.avoid[{index}]"), avoid);
    }
}

fn validate_prior_failures(prior_failures: &PriorFailures, errors: &mut Vec<BriefValidationError>) {
    if prior_failures.notes.is_empty() {
        push_error(
            errors,
            "prior_failures.notes",
            "at least one prior failure note is required",
        );
    }
    for (index, note) in prior_failures.notes.iter().enumerate() {
        push_required(errors, &format!("prior_failures.notes[{index}]"), note);
    }
}

fn push_error(errors: &mut Vec<BriefValidationError>, path: &str, message: &str) {
    errors.push(BriefValidationError {
        path: path.to_owned(),
        message: message.to_owned(),
    });
}

fn format_brief_errors(errors: &[BriefValidationError]) -> String {
    if errors.is_empty() {
        return "no validation details".to_owned();
    }

    errors
        .iter()
        .map(|error| format!("{} {}", error.path, error.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn classify(normalised: &str) -> RequestKind {
    if contains_any(normalised, ANALYSIS_KEYWORDS) {
        return RequestKind::Analysis;
    }
    if contains_any(normalised, REPORT_KEYWORDS) {
        return RequestKind::Report;
    }
    if contains_any(normalised, DASHBOARD_KEYWORDS) {
        return RequestKind::Dashboard;
    }
    RequestKind::Unclassified
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|&n| haystack.contains(n))
}

/// Generate a URL-safe slug from the first few words of a description.
fn slugify(description: &str) -> String {
    let words: Vec<&str> = description.split_whitespace().take(8).collect();
    let raw = words.join(" ");
    raw.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', "-")
        .replace(' ', "-")
        .trim_matches('-')
        .to_string()
}

/// Extract bullet-looking requirements from the description.
fn extract_requirements(description: &str) -> Vec<String> {
    description
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('-') || trimmed.starts_with('*') {
                let without_bullet = trimmed
                    .trim_start_matches('-')
                    .trim_start_matches('*')
                    .trim();
                if without_bullet.is_empty() {
                    None
                } else {
                    Some(without_bullet.to_owned())
                }
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn valid_brief() -> Brief {
        Brief {
            meta: BriefMeta {
                title: "Quarterly Review".to_owned(),
                client: "Acme".to_owned(),
                theme: "protos".to_owned(),
                date: Some("2026-05-29".to_owned()),
                confidential: true,
            },
            walk_away: WalkAway {
                thought: "The platform is ready for focused adoption.".to_owned(),
            },
            audience: vec![Audience {
                role: "CTO".to_owned(),
                cares_about: vec!["technical architecture".to_owned()],
                depth: AudienceDepth::Deep,
            }],
            voice: Voice {
                exemplars: vec![
                    "Direct claim one.".to_owned(),
                    "Direct claim two.".to_owned(),
                    "Direct claim three.".to_owned(),
                    "Direct claim four.".to_owned(),
                    "Direct claim five.".to_owned(),
                ],
                register: "direct-factual".to_owned(),
                forbid: vec!["em-dash".to_owned()],
            },
            arc: Arc {
                opener: "What changed since the last quarterly review?".to_owned(),
                movements: vec!["Tension".to_owned(), "Proof".to_owned()],
                closer: "The platform is ready for focused adoption.".to_owned(),
                emotional_beat: vec!["tension".to_owned(), "confidence".to_owned()],
            },
            receipts: vec![Receipt {
                id: "loc".to_owned(),
                claim: "lines of code".to_owned(),
                value: ReceiptValue::Integer(230_800),
                unit: "count".to_owned(),
                source: ReceiptSource {
                    kind: "file".to_owned(),
                    path: Some("metrics/cloc.json".to_owned()),
                    locator: Some("$.total.code".to_owned()),
                    url: None,
                    detail: None,
                },
                movement: Some("Proof".to_owned()),
            }],
            aesthetic: Aesthetic {
                anchor: "apple-restraint + research-talk-density".to_owned(),
                avoid: vec!["generic gradient".to_owned()],
                density: Density::Balanced,
            },
            prior_failures: PriorFailures {
                notes: vec!["v6 was too plain".to_owned()],
            },
        }
    }

    #[test]
    fn parse_intake_classifies_analysis() {
        let req = parse_intake("analyze the Q3 revenue trends").expect("parse");
        assert_eq!(req.kind, RequestKind::Analysis);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "analyze the Q3 revenue trends");
    }

    #[test]
    fn parse_intake_classifies_report() {
        let req = parse_intake("write a report on customer churn").expect("parse");
        assert_eq!(req.kind, RequestKind::Report);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "write a report on customer churn");
    }

    #[test]
    fn parse_intake_classifies_dashboard() {
        let req = parse_intake("dashboard for server metrics").expect("parse");
        assert_eq!(req.kind, RequestKind::Dashboard);
        assert!(!req.slug.is_empty());
        assert_eq!(req.description, "dashboard for server metrics");
    }

    #[test]
    fn parse_intake_falls_back_to_unclassified() {
        let req = parse_intake("hello world").expect("parse");
        assert_eq!(req.kind, RequestKind::Unclassified);
    }

    #[test]
    fn parse_intake_extracts_requirements() {
        let text = "analyze the data\n- must include charts\n- compare with last year";
        let req = parse_intake(text).expect("parse");
        assert_eq!(req.requirements.len(), 2);
        assert_eq!(
            req.requirements.first().expect("first requirement"),
            "must include charts"
        );
        assert_eq!(
            req.requirements.get(1).expect("second requirement"),
            "compare with last year"
        );
    }

    #[test]
    fn parse_intake_empty_input_errors() {
        let err = parse_intake("   ").expect_err("should fail");
        match err {
            Error::ParseIntake { message } => {
                assert!(message.contains("empty"));
            }
            Error::InvalidBrief { errors } => {
                panic!("unexpected brief validation errors: {errors:?}");
            }
        }
    }

    #[test]
    fn brief_validation_accepts_load_bearing_fields() {
        let brief = valid_brief();
        let report = brief.validate();

        assert!(report.valid);
        assert!(report.errors.is_empty());
        assert_eq!(
            Brief::authoring_axes(),
            [
                AuthoringAxis::ThemeName,
                AuthoringAxis::ComponentChoices,
                AuthoringAxis::Content,
                AuthoringAxis::FactSources,
            ]
        );
    }

    #[test]
    fn brief_validation_rejects_missing_voice_arc_and_receipts() {
        let mut brief = valid_brief();
        brief.voice.exemplars.truncate(2);
        brief.arc.emotional_beat.pop();
        brief.receipts.clear();

        let report = brief.validate();

        assert!(!report.valid);
        let paths: Vec<&str> = report
            .errors
            .iter()
            .map(|error| error.path.as_str())
            .collect();
        assert!(paths.contains(&"voice.exemplars"));
        assert!(paths.contains(&"arc.emotional_beat"));
        assert!(paths.contains(&"receipts"));
    }

    #[test]
    fn validate_brief_returns_structured_errors() {
        let mut brief = valid_brief();
        brief.meta.theme.clear();

        let err = validate_brief(&brief).expect_err("brief should be invalid");

        match err {
            Error::InvalidBrief { errors } => {
                assert!(errors.iter().any(|error| error.path == "meta.theme"));
            }
            Error::ParseIntake { .. } => panic!("expected brief validation error"),
        }
    }
}
