use serde_json::json;

use super::validate_section;

#[test]
fn accepts_openai_api_family_values() {
    for api_family in ["chat-completions", "responses"] {
        let section = json!([
            {
                "name": "openai-cloud",
                "providerType": "openai",
                "apiFamily": api_family,
                "models": ["gpt-4.1"]
            }
        ]);

        assert!(
            validate_section("providers", &section).is_ok(),
            "apiFamily '{api_family}' should validate"
        );
    }
}

#[test]
fn rejects_unknown_openai_api_family() {
    let section = json!([
        {
            "name": "openai-cloud",
            "providerType": "openai",
            "apiFamily": "edits",
            "models": ["gpt-4.1"]
        }
    ]);

    match validate_section("providers", &section) {
        Ok(()) => panic!("apiFamily should fail"),
        Err(err) => assert!(err.to_string().contains("apiFamily 'edits'")),
    }
}
