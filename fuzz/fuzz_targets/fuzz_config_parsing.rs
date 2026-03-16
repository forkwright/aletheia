//! Fuzz target for taxis configuration parsing.
//!
//! Exercises config deserialization from both JSON and TOML, and the
//! `validate_section` function that guards config updates via the API.
//! Targets oversized payloads, unicode, deeply nested structures, and
//! unexpected types in every config section.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // 1. AletheiaConfig from JSON: the runtime config update path.
    //    Tests camelCase rename, default handling, nested struct parsing,
    //    HashMap keys, Vec elements, and enum variants.
    let _ = serde_json::from_slice::<aletheia_taxis::config::AletheiaConfig>(data);

    // 2. AletheiaConfig from TOML: the file-based config loading path.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = toml::from_str::<aletheia_taxis::config::AletheiaConfig>(s);
    }

    // 3. validate_section: exercises all 8 section validators with arbitrary JSON.
    //    This is the API boundary that accepts user-controlled JSON payloads.
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
        static SECTIONS: &[&str] = &[
            "agents",
            "gateway",
            "maintenance",
            "data",
            "embedding",
            "channels",
            "bindings",
            "credential",
            "packs",
            "pricing",
            "sandbox",
            "unknown_section",
        ];

        for section in SECTIONS {
            let _ = aletheia_taxis::validate::validate_section(section, &value);
        }
    }

    // 4. Individual config struct deserialization: tighter surface coverage.
    let _ = serde_json::from_slice::<aletheia_taxis::config::GatewayConfig>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::AgentsConfig>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::ChannelsConfig>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::MaintenanceSettings>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::CredentialConfig>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::EmbeddingSettings>(data);
    let _ = serde_json::from_slice::<aletheia_taxis::config::NousDefinition>(data);

    // 5. JSON roundtrip of default config: ensures serialize/deserialize symmetry.
    if data.first() == Some(&0xFF) {
        let default_config = aletheia_taxis::config::AletheiaConfig::default();
        let json = serde_json::to_vec(&default_config);
        if let Ok(bytes) = json {
            let roundtrip =
                serde_json::from_slice::<aletheia_taxis::config::AletheiaConfig>(&bytes);
            assert!(roundtrip.is_ok(), "default config roundtrip must not fail");
        }
    }
});
