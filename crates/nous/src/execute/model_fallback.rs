//! Registry-backed model fallback for the execute stage.

use hermeneus::error as llm_error;
use hermeneus::fallback::FallbackConfig;
use hermeneus::provider::ProviderRegistry;
use hermeneus::types::{CompletionRequest, CompletionResponse};
use snafu::ResultExt;

use super::resolve::{ProviderAdmission, resolve_admitted_provider};
use crate::error;
use crate::pipeline::PipelineContext;

/// Successful registry-backed fallback completion.
pub(super) struct RegistryFallbackCompletion {
    /// Provider response.
    pub(super) response: CompletionResponse,
    /// Request model that completed successfully.
    pub(super) model: String,
    /// Provider that served the successful request.
    pub(super) provider_name: String,
    /// Deployment target of the successful provider.
    pub(super) deployment_target: hermeneus::provider::DeploymentTarget,
}

/// Execute a completion request with fallback using current-turn sensitivity policy.
pub(super) async fn complete_with_registry_fallback_for_context(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    config: &FallbackConfig,
    ctx: &PipelineContext,
) -> error::Result<RegistryFallbackCompletion> {
    let primary = &request.model;
    let mut last_error = None;
    let mut attempt_errors = Vec::new();
    let mut policy_blocks = Vec::new();

    let models = std::iter::once(primary.clone()).chain(config.fallback_models.iter().cloned());
    for model in models {
        let mut request_for_model = request.clone();
        request_for_model.model.clone_from(&model);

        let provider = match resolve_admitted_provider(ctx, providers, &model) {
            ProviderAdmission::Admitted(provider) => provider,
            ProviderAdmission::Blocked(message) => {
                policy_blocks.push(format!("{model}: {message}"));
                continue;
            }
            ProviderAdmission::Unavailable(message) => {
                attempt_errors.push(format!("{model}: {message}"));
                continue;
            }
            ProviderAdmission::ResolutionError(err) => return Err(err),
        };

        let provider_name = provider.name().to_owned();
        let deployment_target = provider.deployment_target();
        for attempt in 0..config.retries_before_fallback.max(1) {
            let is_primary = model == primary.as_str();
            if is_primary && attempt > 0 {
                tracing::warn!(model = %model, attempt, "retrying primary model");
            } else if !is_primary && attempt == 0 {
                tracing::warn!(
                    primary = %primary,
                    fallback = %model,
                    reason = %last_error.as_ref().map_or("policy block or prior retryable error", |_| "retryable error on previous model"),
                    "falling back to alternative model"
                );
            } else if !is_primary {
                tracing::warn!(model = %model, attempt, "retrying fallback model");
            }

            match provider.complete(&request_for_model).await {
                Ok(response) => {
                    providers.record_success(&provider_name);
                    return Ok(RegistryFallbackCompletion {
                        response,
                        model,
                        provider_name,
                        deployment_target,
                    });
                }
                Err(err) => {
                    providers.record_error(&provider_name, &err);
                    if !err.is_retryable() {
                        return Err(err).context(error::LlmSnafu);
                    }
                    tracing::warn!(
                        model = %model,
                        attempt,
                        error = %err,
                        "fallback chain model failed with retryable error"
                    );
                    attempt_errors.push(format!("{model}: {err}"));
                    last_error = Some(err);
                }
            }
        }
    }

    if let Some(err) = last_error {
        if attempt_errors.is_empty() {
            return Err(err).context(error::LlmSnafu);
        }
        return fallback_chain_failed(&attempt_errors);
    }

    if policy_blocks.is_empty() && !attempt_errors.is_empty() {
        return fallback_chain_failed(&attempt_errors);
    }

    Err(error::PipelineStageSnafu {
        stage: "execute",
        message: admission_failure_message(&policy_blocks, &attempt_errors),
    }
    .build())
}

fn fallback_chain_failed(attempt_errors: &[String]) -> error::Result<RegistryFallbackCompletion> {
    Err(llm_error::ApiRequestSnafu {
        message: format!(
            "connection unavailable: all models in fallback chain failed: {}",
            attempt_errors.join("; ")
        ),
    }
    .build())
    .context(error::LlmSnafu)
}

fn admission_failure_message(policy_blocks: &[String], attempt_errors: &[String]) -> String {
    if policy_blocks.is_empty() {
        "no provider in fallback chain was eligible for the current turn".to_owned()
    } else if attempt_errors.is_empty() {
        format!(
            "no provider in fallback chain may receive the current-turn prompt: {}",
            policy_blocks.join("; ")
        )
    } else {
        format!(
            "no provider in fallback chain may receive the current-turn prompt: {}; unavailable providers: {}",
            policy_blocks.join("; "),
            attempt_errors.join("; ")
        )
    }
}
