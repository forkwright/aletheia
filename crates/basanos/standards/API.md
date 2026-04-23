# API Consistency Standards

## Principle

Interfaces should arrive "as if they could not have been any other." The mechanical test: within a single API surface, conventions should be consistent. Mixed conventions feel worked and create cognitive friction.

## Rules

### API/field-casing

**Severity**: Warn

**Description**: Detects when types in the same crate use both snake_case and camelCase serde field renames.

**Example violation**:
```rust
pub struct User {
    #[serde(rename = "userId")]
    id: String,
}

pub struct Config {
    #[serde(rename = "user_name")]  // ← inconsistent with User
    name: String,
}
```

**Resolution**: Choose one casing convention for the crate and apply it consistently. Common patterns:
- **camelCase**: common for JSON APIs and web services (e.g., `userId`, `userName`)
- **snake_case**: common for internal/config APIs and API-first designs (e.g., `user_id`, `user_name`)

### API/error-variant-naming

**Severity**: Warn

**Description**: Detects inconsistent error variant naming patterns within the same error enum.

**Example violation**:
```rust
pub enum ApiError {
    NotFound,           // adjective pattern
    InvalidUser,        // adjective pattern
    ItemDoesNotExist,   // subject + verb pattern ← inconsistent
}
```

**Resolution**: Choose a naming pattern and apply it consistently within the enum:
- **Adjective pattern**: `NotFound`, `InvalidUser`, `Unauthorized` — names describe the error condition
- **Subject-verb pattern**: `UserNotFound`, `ItemDoesNotExist` — names describe what could not be done
- **Noun-based pattern**: `UserError`, `ItemMissing` — names describe the error category

Pick one and apply throughout the enum.
