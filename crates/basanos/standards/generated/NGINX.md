# NGINX Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [NGINX/upstream-timeout-required](#nginxupstream-timeout-required)

## `NGINX/upstream-timeout-required` {#nginxupstream-timeout-required}

- Severity: `warning`
- Scope: `project:ops`

Every nginx upstream block that uses proxy_pass must set explicit proxy_connect_timeout, proxy_send_timeout, and proxy_read_timeout values. The nginx default values are long enough to cause cascading failures under backend slowdowns. Explicit timeouts bound the failure window and make the policy visible to reviewers.

### Examples

**Good:** Set explicit connect, send, and read timeouts on every upstream block.

```text
upstream backend {
    server 127.0.0.1:8080;
}
location / {
    proxy_connect_timeout 5s;
    proxy_send_timeout    30s;
    proxy_read_timeout    30s;
    proxy_pass http://backend;
}
```

**Bad:** Proxy to an upstream with no timeout directives, relying on nginx defaults.

```text
location / {
    proxy_pass http://127.0.0.1:8080;
    # no timeouts set — defaults may allow minutes of hang
```

