//! CLI provider-choice helpers for generated instance configs.

use crate::error::Result;

pub(crate) const SUPPORTED_PROVIDERS: &str = "anthropic, openai";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CliProvider {
    Anthropic,
    OpenAi,
}

impl CliProvider {
    pub(crate) fn parse(provider: &str) -> Option<Self> {
        match provider {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }

    fn provider_type(self) -> &'static str {
        self.name()
    }

    fn provider_name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic-cloud",
            Self::OpenAi => "openai-cloud",
        }
    }

    fn api_key_env(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
        }
    }
}

pub(crate) fn unsupported_provider_message(provider: &str) -> String {
    format!("unsupported provider: {provider}\nSupported providers: {SUPPORTED_PROVIDERS}")
}

pub(crate) fn init_provider_section(provider: CliProvider, model: &str) -> Option<String> {
    if provider == CliProvider::Anthropic {
        return None;
    }

    let mut doc = toml_edit::DocumentMut::new();
    let mut providers = toml_edit::ArrayOfTables::new();
    providers.push(provider_table(provider, model));
    doc.insert("providers", toml_edit::Item::ArrayOfTables(providers));
    Some(format!("\n# --- LLM providers ---\n{doc}"))
}

pub(crate) fn ensure_provider_model(
    doc: &mut toml_edit::DocumentMut,
    provider: CliProvider,
    model: &str,
) -> Result<()> {
    if provider == CliProvider::Anthropic && doc.get("providers").is_none() {
        return Ok(());
    }

    if doc.get("providers").is_none() {
        doc.insert(
            "providers",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }

    let providers = doc
        .get_mut("providers")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
        .ok_or_else(|| crate::error::Error::msg("providers in config is not an array of tables"))?;

    for entry in providers.iter_mut() {
        if entry.get("providerType").and_then(toml_edit::Item::as_str)
            == Some(provider.provider_type())
        {
            append_model(entry, model)?;
            return Ok(());
        }
    }

    providers.push(provider_table(provider, model));
    Ok(())
}

fn provider_table(provider: CliProvider, model: &str) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table.insert("name", toml_edit::value(provider.provider_name()));
    table.insert("providerType", toml_edit::value(provider.provider_type()));
    if provider == CliProvider::OpenAi {
        table.insert("apiKeyEnv", toml_edit::value(provider.api_key_env()));
        table.insert("apiFamily", toml_edit::value("responses"));
    }
    table.insert("deploymentTarget", toml_edit::value("cloud"));
    table.insert(
        "models",
        toml_edit::Item::Value(toml_edit::Value::Array(model_array(model))),
    );
    table
}

fn append_model(table: &mut toml_edit::Table, model: &str) -> Result<()> {
    if table.get("models").is_none() {
        table.insert(
            "models",
            toml_edit::Item::Value(toml_edit::Value::Array(model_array(model))),
        );
        return Ok(());
    }

    let models = table
        .get_mut("models")
        .and_then(toml_edit::Item::as_value_mut)
        .and_then(toml_edit::Value::as_array_mut)
        .ok_or_else(|| crate::error::Error::msg("providers.models in config is not an array"))?;

    let already_present = models.iter().any(|entry| entry.as_str() == Some(model));
    if !already_present {
        models.push(model);
    }
    Ok(())
}

fn model_array(model: &str) -> toml_edit::Array {
    let mut models = toml_edit::Array::new();
    models.push(model);
    models
}

pub(crate) fn parse_cli_provider(provider: &str) -> Result<CliProvider> {
    CliProvider::parse(provider)
        .ok_or_else(|| crate::error::Error::msg(unsupported_provider_message(provider)))
}

pub(crate) fn validate_cli_provider(provider: &str) -> Result<()> {
    parse_cli_provider(provider).map(|_| ())
}
