# Licensing

Aletheia uses a dual-license model:

| Component | License | Why |
|-----------|---------|-----|
| Runtime, gateway, memory sidecar, prosoche | **AGPL-3.0** | Network service — modifications must be shared |
| SDK, client libraries, contracts, MCP definitions | **Apache-2.0** | Permissive — anyone can build integrations |

## Default

Unless otherwise noted, all code is licensed under the **GNU Affero General Public License v3.0** (see [LICENSE](LICENSE)).

## Apache-2.0 Components

The following directories are licensed under Apache-2.0 (see [licenses/Apache-2.0.txt](licenses/Apache-2.0.txt)):

- `shared/contracts/` — Agent capability contracts
- Future SDK/client libraries when created

Each Apache-2.0 directory contains its own LICENSE file referencing the Apache license.

## Why AGPL for the Runtime

Aletheia runs as a network service. The AGPL ensures that anyone who modifies and deploys the runtime must share their changes — standard GPL wouldn't cover network use. This protects the project while keeping it fully open.

## Why Apache for Integrations

Client libraries and contracts should be usable by anyone without copyleft obligations. Apache-2.0's patent grant and permissive terms make it the right choice for integration points.
