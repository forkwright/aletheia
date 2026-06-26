//! Registry-backed model fallback for the execute stage.

use std::future::Future;
use std::pin::Pin;

use hermeneus::error as llm_error;
use hermeneus::fallback::{FallbackCompletion, FallbackConfig, complete_with_fallback_observed};
use hermeneus::health::ProviderHealth;
use hermeneus::provider::{LlmProvider, ProviderRegistry, ProviderResolutionError, ProviderRoute};
use hermeneus::types::{CompletionRequest, CompletionResponse};

struct RegistryFallbackProvider<'a> {
    providers: &'a ProviderRegistry,
}

impl LlmProvider for RegistryFallbackProvider<'_> {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = llm_error::Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(async move {
            let model = request.model.clone();
            let provider = match self
                .providers
                .resolve_provider(&model, ProviderRoute::ModelOnly)
            {
                Ok(provider) => provider,
                Err(ProviderResolutionError::NoProvider { .. }) => {
                    return Err(llm_error::UnsupportedModelSnafu {
                        model: model.clone(),
                    }
                    .build());
                }
                Err(ProviderResolutionError::ProviderUnavailable { name, health }) => {
                    return Err(llm_error::ApiRequestSnafu {
                        message: format!("provider '{name}' is currently unavailable: {health:?}"),
                    }
                    .build());
                }
            };

            if let Some(health) = self.providers.provider_health(provider.name())
                && matches!(health, ProviderHealth::Down { .. })
            {
                return Err(llm_error::ApiRequestSnafu {
                    message: format!("provider '{}' is currently unavailable", provider.name()),
                }
                .build());
            }

            let provider_name = provider.name().to_owned();
            match provider.complete(request).await {
                Ok(resp) => {
                    self.providers.record_success(&provider_name);
                    Ok(resp)
                }
                Err(e) => {
                    self.providers.record_error(&provider_name, &e);
                    Err(e)
                }
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        &[]
    }

    fn name(&self) -> &'static str {
        "provider-registry"
    }
}

/// Execute a completion request with registry-backed model fallback.
///
/// Returns the successful [`CompletionResponse`] alongside the model identifier
/// that produced it. The model may differ from `request.model` when a fallback
/// model succeeded.
pub(super) async fn complete_with_registry_fallback(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    config: &FallbackConfig,
) -> llm_error::Result<FallbackCompletion> {
    let fallback_provider = RegistryFallbackProvider { providers };
    complete_with_fallback_observed(&fallback_provider, request, config).await
}
