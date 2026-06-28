# Environment Configuration Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [ENVIRONMENT/fail-fast-on-invalid-config](#environmentfail-fast-on-invalid-config)

## `ENVIRONMENT/fail-fast-on-invalid-config` {#environmentfail-fast-on-invalid-config}

- Severity: `error`
- Scope: `universal`
- See also: `PARAMETERS/safe-default-required`

All configuration must be validated at process startup, before serving any traffic. Missing required values, invalid paths, type mismatches, and credential failures produce clear error messages immediately. Discovering broken configuration at 3 AM when a code path first executes is a design defect: the system must fail closed at init, not silently at runtime.

### Examples

**Good:** Validate all configuration at startup and exit with a clear error if any is invalid.

```text
fn main() {
    let config = Config::from_env().expect("invalid configuration at startup");
    tracing::info!(port = config.port, "configuration loaded");
    serve(config).await;
}
```

**Bad:** Discover a missing configuration value the first time a code path needs it.

```text
async fn handle_request(req: Request) -> Response {
    let api_key = std::env::var("API_KEY").ok(); // discovered at 3 AM
```

