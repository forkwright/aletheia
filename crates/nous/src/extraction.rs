//! Bridge from nous to mneme's extraction pipeline via hermeneus providers.

use std::sync::Arc;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};
use aletheia_mneme::extract::{ExtractionError, ExtractionProvider, LlmCallSnafu};
use aletheia_mneme::skills::extract::{
    LlmCallSnafu as SkillLlmCallSnafu, SkillExtractionError, SkillExtractionProvider,
};
use snafu::OptionExt;

/// Bridges hermeneus `ProviderRegistry` to mneme's `ExtractionProvider` trait.
pub(crate) struct HermeneusExtractionProvider {
    providers: Arc<ProviderRegistry>,
    model: String,
}

impl HermeneusExtractionProvider {
    pub(crate) fn new(providers: Arc<ProviderRegistry>, model: &str) -> Self {
        Self {
            providers,
            model: model.to_owned(),
        }
    }
}

impl ExtractionProvider for HermeneusExtractionProvider {
    fn complete(&self, system: &str, user_message: &str) -> Result<String, ExtractionError> {
        let request = CompletionRequest {
            model: self.model.clone(),
            system: Some(system.to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_message.to_owned()),
            }],
            max_tokens: 4096,
            tools: Vec::new(),
            temperature: None,
            thinking: None,
            stop_sequences: Vec::new(),
            ..Default::default()
        };

        let provider = self.providers.find_provider(&self.model).ok_or_else(|| {
            LlmCallSnafu {
                message: format!("no provider for model {}", self.model),
            }
            .build()
        })?;

        let response = provider.complete(&request).map_err(|e| {
            LlmCallSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        response
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .context(LlmCallSnafu {
                message: "no text content in extraction response",
            })
    }
}

/// Bridges hermeneus `ProviderRegistry` to mneme's [`SkillExtractionProvider`] trait.
pub(crate) struct HermeneusSkillExtractionProvider {
    providers: Arc<ProviderRegistry>,
    model: String,
}

impl HermeneusSkillExtractionProvider {
    pub(crate) fn new(providers: Arc<ProviderRegistry>, model: &str) -> Self {
        Self {
            providers,
            model: model.to_owned(),
        }
    }
}

impl SkillExtractionProvider for HermeneusSkillExtractionProvider {
    fn complete(&self, system: &str, user_message: &str) -> Result<String, SkillExtractionError> {
        let request = CompletionRequest {
            model: self.model.clone(),
            system: Some(system.to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_message.to_owned()),
            }],
            max_tokens: 2048,
            tools: Vec::new(),
            temperature: None,
            thinking: None,
            stop_sequences: Vec::new(),
            ..Default::default()
        };

        let provider = self.providers.find_provider(&self.model).ok_or_else(|| {
            SkillLlmCallSnafu {
                message: format!("no provider for model {}", self.model),
            }
            .build()
        })?;

        let response = provider.complete(&request).map_err(|e| {
            let msg = e.to_string();
            SkillLlmCallSnafu { message: msg }.build()
        })?;

        response
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .context(SkillLlmCallSnafu {
                message: "no text content in skill extraction response",
            })
    }
}
