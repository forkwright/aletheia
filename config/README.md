# Configuration security defaults

## Gateway auth and bind

`gateway.auth.mode = "none"` is only supported with a loopback bind such as
`gateway.bind = "localhost"` or `127.0.0.1`.

For LAN or tailnet access, use `gateway.auth.mode = "token"` or
`gateway.auth.mode = "jwt"` with `gateway.bind = "lan"` instead.

Startup refuses `auth.mode = "none"` on non-loopback binds unless the operator
sets `ALETHEIA_ALLOW_AUTH_NONE_LAN=1`. That override is an emergency escape
hatch, not a supported default, and startup logs a warning when it is used so it
is visible in service journals.
