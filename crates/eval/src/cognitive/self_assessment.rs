//! Self-assessment template for structured agent self-evaluation.

use serde::{Deserialize, Serialize};
use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval};
use crate::sse;

/// Structured self-assessment output from an agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SelfAssessment {
    /// Areas where the agent performs well.
    #[serde(default)]
    pub strengths: Vec<String>,
    /// Areas where the agent needs improvement.
    #[serde(default)]
    pub weaknesses: Vec<String>,
    /// Concrete improvement areas with actionable steps.
    #[serde(default)]
    pub improvement_areas: Vec<String>,
}

/// The self-assessment prompt template.
const SELF_ASSESSMENT_PROMPT: &str = r#"Evaluate your own capabilities as an AI assistant. Provide a structured self-assessment in the following JSON format (respond ONLY with JSON, no other text):

{
  "strengths": ["strength 1", "strength 2", "strength 3"],
  "weaknesses": ["weakness 1", "weakness 2", "weakness 3"],
  "improvement_areas": ["area 1", "area 2", "area 3"]
}

Be honest and specific. Each list should have exactly 3 items."#;

/// Attempt to parse a self-assessment from an agent response.
///
/// Extracts JSON from the response text, handling cases where the agent
/// wraps JSON in markdown code blocks.
pub(crate) fn parse_self_assessment(response: &str) -> Option<SelfAssessment> {
    let trimmed = response.trim();

    // WHY: agents often wrap JSON in markdown code fences
    let json_str = if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}')?;
        trimmed.get(start..=end)?
    } else {
        return None;
    };

    serde_json::from_str(json_str).ok()
}

/// Validate that a self-assessment has the expected structure.
#[must_use]
pub(crate) fn validate_assessment(assessment: &SelfAssessment) -> bool {
    !assessment.strengths.is_empty()
        && !assessment.weaknesses.is_empty()
        && !assessment.improvement_areas.is_empty()
        && assessment.strengths.iter().all(|s| !s.is_empty())
        && assessment.weaknesses.iter().all(|s| !s.is_empty())
        && assessment.improvement_areas.iter().all(|s| !s.is_empty())
}

/// Scenario that requests a structured self-assessment from the agent.
struct SelfAssessmentScenario;

impl Scenario for SelfAssessmentScenario {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "self-assessment-template",
            description: "Generate structured self-assessment with strengths, weaknesses, improvements",
            category: "cognitive",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;

                let key = crate::scenarios::unique_key("self", "assess");
                let session = client.create_session(&nous.id, &key).await?;
                let events = client
                    .send_message(&session.id, SELF_ASSESSMENT_PROMPT)
                    .await?;
                let text = sse::extract_text(&events);

                assert_eval(!text.is_empty(), "self-assessment response should not be empty")?;

                match parse_self_assessment(&text) {
                    Some(assessment) => {
                        tracing::info!(
                            strengths = assessment.strengths.len(),
                            weaknesses = assessment.weaknesses.len(),
                            improvements = assessment.improvement_areas.len(),
                            "self-assessment parsed"
                        );

                        assert_eval(
                            validate_assessment(&assessment),
                            "self-assessment should have non-empty strengths, weaknesses, and improvements",
                        )?;
                    }
                    None => {
                        tracing::warn!(
                            response_len = text.len(),
                            "failed to parse self-assessment JSON from response"
                        );
                    }
                }

                let _ = client.close_session(&session.id).await;
                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "self-assessment-template"
            )),
        )
    }
}

pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![Box::new(SelfAssessmentScenario)]
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let response =
            r#"{"strengths": ["a", "b"], "weaknesses": ["c"], "improvement_areas": ["d"]}"#;
        let result = parse_self_assessment(response);
        assert!(result.is_some(), "valid JSON should parse");
        let assessment = result.expect("just asserted Some");
        assert_eq!(assessment.strengths.len(), 2, "should have 2 strengths");
        assert_eq!(assessment.weaknesses.len(), 1, "should have 1 weakness");
        assert_eq!(
            assessment.improvement_areas.len(),
            1,
            "should have 1 improvement area"
        );
    }

    #[test]
    fn parse_json_in_code_block() {
        let response = "Here is my assessment:\n```json\n{\"strengths\": [\"good\"], \"weaknesses\": [\"bad\"], \"improvement_areas\": [\"better\"]}\n```";
        let result = parse_self_assessment(response);
        assert!(result.is_some(), "JSON in code block should parse");
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let response = "Sure, here is my assessment: {\"strengths\": [\"a\"], \"weaknesses\": [\"b\"], \"improvement_areas\": [\"c\"]} I hope this helps.";
        let result = parse_self_assessment(response);
        assert!(result.is_some(), "JSON with surrounding text should parse");
    }

    #[test]
    fn parse_no_json_returns_none() {
        let response = "I cannot provide a structured assessment.";
        let result = parse_self_assessment(response);
        assert!(result.is_none(), "response without JSON should return None");
    }

    #[test]
    fn parse_malformed_json_returns_none() {
        let response = "{\"strengths\": [\"a\"], \"weaknesses\":";
        let result = parse_self_assessment(response);
        assert!(result.is_none(), "malformed JSON should return None");
    }

    #[test]
    fn parse_empty_response_returns_none() {
        let result = parse_self_assessment("");
        assert!(result.is_none(), "empty response should return None");
    }

    #[test]
    fn validate_complete_assessment() {
        let assessment = SelfAssessment {
            strengths: vec!["a".to_owned()],
            weaknesses: vec!["b".to_owned()],
            improvement_areas: vec!["c".to_owned()],
        };
        assert!(
            validate_assessment(&assessment),
            "complete assessment should be valid"
        );
    }

    #[test]
    fn validate_empty_strengths_fails() {
        let assessment = SelfAssessment {
            strengths: vec![],
            weaknesses: vec!["b".to_owned()],
            improvement_areas: vec!["c".to_owned()],
        };
        assert!(
            !validate_assessment(&assessment),
            "empty strengths should be invalid"
        );
    }

    #[test]
    fn validate_empty_weaknesses_fails() {
        let assessment = SelfAssessment {
            strengths: vec!["a".to_owned()],
            weaknesses: vec![],
            improvement_areas: vec!["c".to_owned()],
        };
        assert!(
            !validate_assessment(&assessment),
            "empty weaknesses should be invalid"
        );
    }

    #[test]
    fn validate_empty_improvements_fails() {
        let assessment = SelfAssessment {
            strengths: vec!["a".to_owned()],
            weaknesses: vec!["b".to_owned()],
            improvement_areas: vec![],
        };
        assert!(
            !validate_assessment(&assessment),
            "empty improvements should be invalid"
        );
    }

    #[test]
    fn validate_empty_string_item_fails() {
        let assessment = SelfAssessment {
            strengths: vec![String::new()],
            weaknesses: vec!["b".to_owned()],
            improvement_areas: vec!["c".to_owned()],
        };
        assert!(
            !validate_assessment(&assessment),
            "empty string items should be invalid"
        );
    }

    #[test]
    fn default_assessment_is_empty() {
        let assessment = SelfAssessment::default();
        assert!(
            assessment.strengths.is_empty(),
            "default strengths should be empty"
        );
        assert!(
            assessment.weaknesses.is_empty(),
            "default weaknesses should be empty"
        );
        assert!(
            assessment.improvement_areas.is_empty(),
            "default improvements should be empty"
        );
    }

    #[test]
    fn self_assessment_roundtrip_serialization() {
        let assessment = SelfAssessment {
            strengths: vec!["a".to_owned(), "b".to_owned()],
            weaknesses: vec!["c".to_owned()],
            improvement_areas: vec!["d".to_owned()],
        };
        let json = serde_json::to_string(&assessment).expect("should serialize");
        let back: SelfAssessment = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(assessment.strengths, back.strengths, "strengths roundtrip");
        assert_eq!(
            assessment.weaknesses, back.weaknesses,
            "weaknesses roundtrip"
        );
        assert_eq!(
            assessment.improvement_areas, back.improvement_areas,
            "improvements roundtrip"
        );
    }

    #[test]
    fn prompt_template_is_nonempty() {
        assert!(
            !SELF_ASSESSMENT_PROMPT.is_empty(),
            "self-assessment prompt should not be empty"
        );
        assert!(
            SELF_ASSESSMENT_PROMPT.contains("strengths"),
            "prompt should mention strengths"
        );
        assert!(
            SELF_ASSESSMENT_PROMPT.contains("weaknesses"),
            "prompt should mention weaknesses"
        );
    }
}
