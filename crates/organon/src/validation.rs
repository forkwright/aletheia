//! Gnomon tool validation contracts for pre/post-dispatch validation.
//!
//! Provides [`ToolContract`] for defining tool input/output schemas and
//! permissions, [`ContractRegistry`] for storing contracts, and validation
//! functions for checking tool calls against contracts.

use std::collections::HashMap;

use serde_json::Value;
use snafu::Snafu;

/// A contract defining the schema and constraints for a tool.
#[derive(Debug, Clone)]
pub struct ToolContract {
    /// The name of the tool this contract applies to.
    pub tool_name: String,
    /// JSON Schema for validating tool inputs.
    pub input_schema: Value,
    /// Optional JSON Schema for validating tool outputs.
    pub output_schema: Option<Value>,
    /// List of required permissions to invoke this tool.
    pub required_permissions: Vec<String>,
    /// Maximum allowed cost in USD for this tool call.
    pub max_cost_usd: Option<f64>,
}

/// Result of validating a tool call against its contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationResult {
    /// The tool call is valid according to the contract.
    Valid,
    /// The tool call violates one or more contract constraints.
    Invalid {
        /// List of validation violation messages.
        violations: Vec<String>,
    },
}

impl ValidationResult {
    /// Returns true if the validation passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Returns true if the validation failed.
    #[must_use]
    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    /// Returns the violations if invalid, or an empty vec if valid.
    #[must_use]
    pub fn violations(&self) -> Vec<String> {
        match self {
            Self::Valid => Vec::new(),
            Self::Invalid { violations } => violations.clone(),
        }
    }
}

/// Errors that can occur during validation operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum ValidationError {
    /// The tool contract was not found in the registry.
    #[snafu(display("contract not found for tool: {tool_name}"))]
    ContractNotFound {
        tool_name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The input schema is malformed.
    #[snafu(display("invalid input schema: {reason}"))]
    InvalidSchema {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Registry for storing and retrieving tool contracts.
#[derive(Debug, Default)]
pub struct ContractRegistry {
    contracts: HashMap<String, ToolContract>,
}

impl ContractRegistry {
    /// Create an empty contract registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
        }
    }

    /// Register a tool contract.
    ///
    /// If a contract for the same tool name already exists, it will be replaced.
    pub fn register(&mut self, contract: ToolContract) {
        self.contracts.insert(contract.tool_name.clone(), contract);
    }

    /// Retrieve a contract by tool name.
    #[must_use]
    pub fn get(&self, tool_name: &str) -> Option<&ToolContract> {
        self.contracts.get(tool_name)
    }

    /// Check if a contract exists for the given tool name.
    #[must_use]
    pub fn contains(&self, tool_name: &str) -> bool {
        self.contracts.contains_key(tool_name)
    }

    /// Remove a contract from the registry.
    ///
    /// Returns the removed contract if it existed.
    pub fn remove(&mut self, tool_name: &str) -> Option<ToolContract> {
        self.contracts.remove(tool_name)
    }

    /// Get the number of registered contracts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }

    /// Validate a tool call against the registered contract.
    ///
    /// Returns [`ValidationResult::Valid`] if no contract is registered for the tool
    /// (permissive default), or if the input passes validation.
    pub fn validate_call(&self, tool_name: &str, input: &Value) -> ValidationResult {
        let Some(contract) = self.contracts.get(tool_name) else {
            // Permissive default: no contract means no validation
            return ValidationResult::Valid;
        };
        validate_input(contract, input)
    }

    /// Validate a tool output against the registered contract.
    ///
    /// Returns [`ValidationResult::Valid`] if no contract is registered for the tool,
    /// or if the contract has no output schema, or if the output passes validation.
    pub fn validate_output(&self, tool_name: &str, output: &Value) -> ValidationResult {
        let Some(contract) = self.contracts.get(tool_name) else {
            return ValidationResult::Valid;
        };
        crate::validation::validate_output(contract, output)
    }

    /// Get all registered tool names.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&str> {
        self.contracts.keys().map(String::as_str).collect()
    }

    /// Clear all registered contracts.
    pub fn clear(&mut self) {
        self.contracts.clear();
    }
}

/// Validate tool input against its contract schema.
///
/// Performs basic JSON Schema validation including:
/// - Type checking for primitive types (string, number, integer, boolean, array, object)
/// - Required field validation
/// - Enum value validation
/// - Nested object validation
pub fn validate_input(contract: &ToolContract, input: &Value) -> ValidationResult {
    let mut violations = Vec::new();

    // Input must be an object
    let Some(_input_obj) = input.as_object() else {
        return ValidationResult::Invalid {
            violations: vec!["input must be a JSON object".to_string()],
        };
    };

    // Validate against the schema
    validate_value_against_schema(
        input,
        &contract.input_schema,
        "input",
        &mut violations,
    );

    if violations.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid { violations }
    }
}

/// Validate tool output against its contract schema.
///
/// If the contract has no output schema, returns [`ValidationResult::Valid`].
/// Otherwise performs the same validation as [`validate_input`].
pub fn validate_output(contract: &ToolContract, output: &Value) -> ValidationResult {
    let Some(ref schema) = contract.output_schema else {
        return ValidationResult::Valid;
    };

    let mut violations = Vec::new();
    validate_value_against_schema(output, schema, "output", &mut violations);

    if violations.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid { violations }
    }
}

/// Pre-dispatch validation hook.
///
/// Checks a tool call against the registry before execution.
/// This is the main entry point for energeia integration.
///
/// # Errors
///
/// Returns an error if the contract exists but validation fails.
/// Returns `Ok(())` if no contract exists (permissive default).
pub fn validate_before_dispatch(
    registry: &ContractRegistry,
    tool_name: &str,
    input: &Value,
) -> Result<(), ValidationError> {
    let result = registry.validate_call(tool_name, input);
    match result {
        ValidationResult::Valid => Ok(()),
        ValidationResult::Invalid { violations } => {
            // Check if contract exists - if not, it's valid (permissive)
            if !registry.contains(tool_name) {
                return Ok(());
            }
            Err(ValidationError::InvalidSchema {
                reason: violations.join("; "),
                location: snafu::location!(),
            })
        }
    }
}

/// Internal function to validate a value against a JSON schema.
fn validate_value_against_schema(
    value: &Value,
    schema: &Value,
    path: &str,
    violations: &mut Vec<String>,
) {
    // Handle schema type validation
    if let Some(type_val) = schema.get("type") {
        let schema_type = type_val.as_str().unwrap_or("object");
        validate_type(value, schema_type, path, violations);
    }

    // Handle object properties validation
    if value.is_object() && schema.get("properties").is_some() {
        let props = schema.get("properties").and_then(Value::as_object);
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let (Some(value_obj), Some(props)) = (value.as_object(), props) {
            // Check required fields
            for &req_field in &required {
                if !value_obj.contains_key(req_field) {
                    violations.push(format!("{path}.{req_field}: missing required field"));
                }
            }

            // Validate each property
            for (prop_name, prop_value) in value_obj {
                if let Some(prop_schema) = props.get(prop_name) {
                    let prop_path = format!("{path}.{prop_name}");
                    validate_value_against_schema(prop_value, prop_schema, &prop_path, violations);
                }
            }
        }
    }

    // Handle array validation
    if value.is_array() && schema.get("items").is_some() {
        if let (Some(items), Some(arr)) = (schema.get("items"), value.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let item_path = format!("{path}[{idx}]");
                validate_value_against_schema(item, items, &item_path, violations);
            }
        }
    }

    // Handle enum validation
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        let valid = enum_vals.iter().any(|v| v == value);
        if !valid {
            let allowed: Vec<String> = enum_vals
                .iter()
                .map(|v| v.to_string())
                .collect();
            violations.push(format!(
                "{path}: value must be one of [{}]",
                allowed.join(", ")
            ));
        }
    }
}

/// Validate that a value matches the expected JSON Schema type.
fn validate_type(value: &Value, expected_type: &str, path: &str, violations: &mut Vec<String>) {
    let actual_type = match value {
        Value::String(_) => "string",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "null",
    };

    // Type compatibility checking
    let compatible = match expected_type {
        "string" => matches!(value, Value::String(_)),
        "number" => matches!(value, Value::Number(_)),
        "integer" => matches!(value, Value::Number(n) if n.is_i64() || n.is_u64()),
        "boolean" => matches!(value, Value::Bool(_)),
        "array" => matches!(value, Value::Array(_)),
        "object" => matches!(value, Value::Object(_)),
        "null" => matches!(value, Value::Null),
        _ => true, // Unknown types pass through
    };

    if !compatible {
        violations.push(format!(
            "{path}: expected type '{expected_type}', got '{actual_type}'"
        ));
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn create_test_contract() -> ToolContract {
        ToolContract {
            tool_name: "test_tool".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The name parameter"
                    },
                    "count": {
                        "type": "integer",
                        "description": "An integer count"
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "A boolean flag"
                    },
                    "ratio": {
                        "type": "number",
                        "description": "A float ratio"
                    },
                    "tags": {
                        "type": "array",
                        "description": "List of tags"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["active", "inactive"]
                    }
                },
                "required": ["name"]
            }),
            output_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "result": {
                        "type": "string"
                    }
                },
                "required": ["result"]
            })),
            required_permissions: vec!["test:execute".to_string()],
            max_cost_usd: Some(1.0),
        }
    }

    #[test]
    fn test_validate_input_valid() {
        let contract = create_test_contract();
        let input = serde_json::json!({
            "name": "test",
            "count": 42,
            "enabled": true,
            "ratio": 3.14,
            "tags": ["a", "b"],
            "status": "active"
        });

        let result = validate_input(&contract, &input);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_input_missing_required() {
        let contract = create_test_contract();
        let input = serde_json::json!({
            "count": 42
        });

        let result = validate_input(&contract, &input);
        assert!(result.is_invalid());
        let violations = result.violations();
        assert!(violations.iter().any(|v| v.contains("missing required field")));
    }

    #[test]
    fn test_validate_input_type_mismatch() {
        let contract = create_test_contract();
        let input = serde_json::json!({
            "name": "test",
            "count": "not an integer"
        });

        let result = validate_input(&contract, &input);
        assert!(result.is_invalid());
        let violations = result.violations();
        assert!(violations.iter().any(|v| v.contains("expected type 'integer'")));
    }

    #[test]
    fn test_validate_input_enum_violation() {
        let contract = create_test_contract();
        let input = serde_json::json!({
            "name": "test",
            "status": "invalid_status"
        });

        let result = validate_input(&contract, &input);
        assert!(result.is_invalid());
        let violations = result.violations();
        assert!(violations.iter().any(|v| v.contains("must be one of")));
    }

    #[test]
    fn test_validate_input_not_object() {
        let contract = create_test_contract();
        let input = serde_json::json!("not an object");

        let result = validate_input(&contract, &input);
        assert!(result.is_invalid());
        assert!(result.violations()[0].contains("must be a JSON object"));
    }

    #[test]
    fn test_validate_output_valid() {
        let contract = create_test_contract();
        let output = serde_json::json!({
            "result": "success"
        });

        let result = validate_output(&contract, &output);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_output_missing_required() {
        let contract = create_test_contract();
        let output = serde_json::json!({});

        let result = validate_output(&contract, &output);
        assert!(result.is_invalid());
    }

    #[test]
    fn test_validate_output_no_schema() {
        let mut contract = create_test_contract();
        contract.output_schema = None;
        let output = serde_json::json!({ "anything": "goes" });

        let result = validate_output(&contract, &output);
        assert!(result.is_valid());
    }

    #[test]
    fn test_contract_registry() {
        let mut registry = ContractRegistry::new();
        let contract = create_test_contract();

        assert!(registry.is_empty());
        assert!(!registry.contains("test_tool"));

        registry.register(contract);

        assert_eq!(registry.len(), 1);
        assert!(registry.contains("test_tool"));
        assert!(registry.get("test_tool").is_some());
        assert!(registry.get("unknown").is_none());

        // Test validate_call
        let valid_input = serde_json::json!({ "name": "test" });
        let result = registry.validate_call("test_tool", &valid_input);
        assert!(result.is_valid());

        let invalid_input = serde_json::json!({});
        let result = registry.validate_call("test_tool", &invalid_input);
        assert!(result.is_invalid());

        // Unknown tools pass validation (permissive default)
        let result = registry.validate_call("unknown_tool", &valid_input);
        assert!(result.is_valid());

        // Test tool_names
        let names = registry.tool_names();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"test_tool"));

        // Test remove
        let removed = registry.remove("test_tool");
        assert!(removed.is_some());
        assert!(registry.is_empty());
    }

    #[test]
    fn test_validate_before_dispatch_success() {
        let mut registry = ContractRegistry::new();
        let contract = create_test_contract();
        registry.register(contract);

        let input = serde_json::json!({ "name": "test" });
        let result = validate_before_dispatch(&registry, "test_tool", &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_before_dispatch_unknown_tool() {
        let registry = ContractRegistry::new();
        let input = serde_json::json!({ "name": "test" });
        // Unknown tools pass validation (permissive default)
        let result = validate_before_dispatch(&registry, "unknown_tool", &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_before_dispatch_invalid() {
        let mut registry = ContractRegistry::new();
        let contract = create_test_contract();
        registry.register(contract);

        let input = serde_json::json!({}); // Missing required "name"
        let result = validate_before_dispatch(&registry, "test_tool", &input);
        assert!(result.is_err());
    }

    #[test]
    fn test_number_vs_integer() {
        let contract = ToolContract {
            tool_name: "num_test".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "int_val": { "type": "integer" },
                    "num_val": { "type": "number" }
                }
            }),
            output_schema: None,
            required_permissions: vec![],
            max_cost_usd: None,
        };

        // Integer field with integer value - valid
        let input = serde_json::json!({ "int_val": 42, "num_val": 3.14 });
        let result = validate_input(&contract, &input);
        assert!(result.is_valid());

        // Integer field with float value - invalid
        let input = serde_json::json!({ "int_val": 3.14 });
        let result = validate_input(&contract, &input);
        assert!(result.is_invalid());

        // Number field with integer value - valid (integers are numbers)
        let input = serde_json::json!({ "num_val": 42 });
        let result = validate_input(&contract, &input);
        assert!(result.is_valid());
    }

    #[test]
    fn test_nested_object_validation() {
        let contract = ToolContract {
            tool_name: "nested_test".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "config": {
                        "type": "object",
                        "properties": {
                            "host": { "type": "string" },
                            "port": { "type": "integer" }
                        },
                        "required": ["host"]
                    }
                }
            }),
            output_schema: None,
            required_permissions: vec![],
            max_cost_usd: None,
        };

        let valid_input = serde_json::json!({
            "config": {
                "host": "localhost",
                "port": 8080
            }
        });
        let result = validate_input(&contract, &valid_input);
        assert!(result.is_valid());

        let invalid_input = serde_json::json!({
            "config": {
                "port": "not an integer"
            }
        });
        let result = validate_input(&contract, &invalid_input);
        assert!(result.is_invalid());
        let violations = result.violations();
        assert!(violations.iter().any(|v| v.contains("missing required field")));
        assert!(violations.iter().any(|v| v.contains("expected type 'integer'")));
    }

    #[test]
    fn test_array_validation() {
        let contract = ToolContract {
            tool_name: "array_test".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                }
            }),
            output_schema: None,
            required_permissions: vec![],
            max_cost_usd: None,
        };

        let valid_input = serde_json::json!({
            "items": ["a", "b", "c"]
        });
        let result = validate_input(&contract, &valid_input);
        assert!(result.is_valid());

        let invalid_input = serde_json::json!({
            "items": ["a", 42, "c"]
        });
        let result = validate_input(&contract, &invalid_input);
        assert!(result.is_invalid());
        let violations = result.violations();
        assert!(violations.iter().any(|v| v.contains("expected type 'string'")));
    }

    #[test]
    fn test_validation_result_methods() {
        let valid = ValidationResult::Valid;
        assert!(valid.is_valid());
        assert!(!valid.is_invalid());
        assert!(valid.violations().is_empty());

        let invalid = ValidationResult::Invalid {
            violations: vec!["error1".to_string(), "error2".to_string()],
        };
        assert!(!invalid.is_valid());
        assert!(invalid.is_invalid());
        assert_eq!(invalid.violations().len(), 2);
    }

    #[test]
    fn test_contract_registry_validate_output() {
        let mut registry = ContractRegistry::new();
        let contract = create_test_contract();
        registry.register(contract);

        let valid_output = serde_json::json!({ "result": "success" });
        let result = registry.validate_output("test_tool", &valid_output);
        assert!(result.is_valid());

        let invalid_output = serde_json::json!({});
        let result = registry.validate_output("test_tool", &invalid_output);
        assert!(result.is_invalid());

        // Unknown tool passes
        let result = registry.validate_output("unknown", &valid_output);
        assert!(result.is_valid());
    }
}
