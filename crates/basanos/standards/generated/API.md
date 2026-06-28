# API Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [API/direct-process-exit](#apidirect-process-exit)
- [API/error-no-request-id](#apierror-no-request-id)
- [API/internal-inconsistency](#apiinternal-inconsistency)
- [API/mixed-casing](#apimixed-casing)

## `API/direct-process-exit` {#apidirect-process-exit}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `API/direct-process-exit`
- See also: `RUST/return-unit`

Library and API code must not call `std::process::exit` directly. Return an error or route shutdown through an explicit boundary so destructors, cleanup hooks, and callers can handle failure consistently.

### Examples

**Good:** Propagate failure through the API boundary.

```text
fn main() -> Result<(), AppError> { run()?; Ok(()) }
```

**Bad:** Abort process cleanup from library or API code.

```text
std::process::exit(1);
```

## `API/error-no-request-id` {#apierror-no-request-id}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `API/error-no-request-id`
- See also: `API/internal-inconsistency`

Error response types that implement `IntoResponse` must include a `request_id` field. The identifier gives operators and clients a stable handle for tracing the failing request through logs and support workflows.

### Examples

**Good:** Include the request identifier in error responses.

```text
struct ErrorResponse { request_id: String, message: String }
```

**Bad:** Return an error response that cannot be traced to a request.

```text
struct ErrorResponse { message: String }
```

## `API/internal-inconsistency` {#apiinternal-inconsistency}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `API/internal-inconsistency`

API surfaces must not mix conventions for similar operations within the same file. Choose one pagination style, one error envelope shape, and one SSE event naming convention so the interface reads as one composed surface.

### Examples

**Good:** Use one pagination convention within a file.

```text
struct ListEventsQuery { cursor: Option<String>, limit: usize }
```

**Bad:** Mix pagination conventions in the same API surface.

```text
struct ListEventsQuery { offset: usize, cursor: Option<String> }
```

## `API/mixed-casing` {#apimixed-casing}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `API/mixed-casing`
- See also: `API/internal-inconsistency`

API files must not mix serde `rename_all` casing strategies. Pick one casing convention per API surface so consumers do not have to handle both camelCase and snake_case shapes from the same interface.

### Examples

**Good:** Use one serde rename_all convention for an API surface.

```text
#[serde(rename_all = "camelCase")]
struct ApiResponse { request_id: String }
```

**Bad:** Mix serde casing conventions in the same API file.

```text
#[serde(rename_all = "camelCase")]
struct ApiResponse { request_id: String }
#[serde(rename_all = "snake_case")]
struct ApiError { request_id: String }
```

