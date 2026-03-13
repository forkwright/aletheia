---
created: 2026-03-11
updated: 2026-03-11
tags:
  - standards
  - nix
  - infrastructure
---

# Nix Language & NixOS Standards

> Companion to `standards/RUST.md`. This covers Nix the language, flake conventions, NixOS module patterns, and Aletheia-specific packaging decisions.

---

## Table of Contents

1. [Philosophy](#1-philosophy)
2. [Language Fundamentals](#2-language-fundamentals)
3. [Style & Formatting](#3-style--formatting)
4. [Flake Structure](#4-flake-structure)
5. [Module Patterns](#5-module-patterns)
6. [Derivation & Packaging](#6-derivation--packaging)
7. [Anti-Patterns](#7-anti-patterns)
8. [Our Conventions](#8-our-conventions)
9. [Tooling](#9-tooling)
10. [Reference](#10-reference)

---

## 1. Philosophy

Nix is a **purely functional, lazily-evaluated, dynamically typed** language purpose-built for declarative software packaging and system configuration. Think "JSON with functions" or "Haskell without types."

Core mental model shift from imperative Linux:

| Imperative (Fedora/Ubuntu) | Declarative (NixOS) |
|---|---|
| `sudo dnf install foo` | Add `foo` to `environment.systemPackages` |
| Edit `/etc/foo.conf` | Set `services.foo.settings = { ... }` |
| State accumulates over time | State is defined in code, rebuilt from scratch |
| Rollback = hope + backups | Rollback = `nixos-rebuild switch --rollback` |
| "Works on my machine" | Reproducible by definition (same inputs → same outputs) |

**Why this matters for Aletheia:** One flake, multiple deployment targets — server (worker-node), desktop (Metis replacement), USB recovery stick. The system state IS a git commit.

---

## 2. Language Fundamentals

### Types

Nix has very few types. This is intentional — simplicity enables reproducibility.

**Primitive types:**

| Type | Examples | Notes |
|------|----------|-------|
| String | `"hello"`, `''multi-line''` | Interpolation with `${}`. Concatenation with `+`. |
| Boolean | `true`, `false` | Only booleans work in `if`. `null` is NOT falsy. |
| Integer | `42`, `-1` | |
| Float | `3.14` | Coerces with integers automatically. |
| Null | `null` | Distinct from `false`. Signifies absence. |
| Path | `./foo.nix`, `/etc/nixos` | Built-in type, not a string. Important for flake purity. |

**Compound types:**

| Type | Syntax | Notes |
|------|--------|-------|
| Attribute set (attrset) | `{ key = value; }` | Semicolons required. Key-value pairs. The fundamental data structure. |
| List | `[ 1 "two" 3 ]` | Space-separated. Heterogeneous. Concatenation with `++`. |

**Functions:**

```nix
# Single argument, single return value. ALWAYS.
x: x + 1

# Application uses space (not parentheses)
(x: x + 1) 2    # => 3

# Multi-argument via currying
x: y: x + y
# (x: y: x + y) 1 2  => 3

# Attrset destructuring (most common pattern)
{ foo, bar }: foo + bar

# With default values
{ foo, bar ? "default" }: foo + bar

# With catch-all for extra args
{ foo, bar, ... }: foo + bar
```

### Key Expressions

**`let ... in`** — Local bindings. The workhorse of factoring out code:

```nix
let
  pkgs = import nixpkgs { system = "x86_64-linux"; };
  version = "1.0.0";
in
  pkgs.mkShell { name = "my-shell-${version}"; }
```

**`if ... then ... else`** — Everything is an expression. `if` returns a value:

```nix
{
  editor = if useVim then "vim" else "nano";
}
```

**`inherit`** — Shorthand for `x = x` in attrsets. NOT OOP inheritance:

```nix
let
  foo = "value";
in {
  inherit foo;          # equivalent to: foo = foo;
  inherit (pkgs) git;   # equivalent to: git = pkgs.git;
}
```

**`with`** — Brings attrset keys into scope. **Use sparingly** (see Anti-Patterns):

```nix
with pkgs; [ git curl jq ]
# equivalent to: [ pkgs.git pkgs.curl pkgs.jq ]
```

**`//`** — Shallow merge of attrsets. Right takes precedence:

```nix
{ a = 1; b = { x = 1; }; } // { b = 2; c = 3; }
# => { a = 1; b = 2; c = 3; }
# WARNING: b's nested attrset is gone. Merge is SHALLOW.
```

### Laziness

Nix is lazily evaluated. Values are only computed when needed. This enables:
- Self-referencing structures (`lib.fix`, `rec`)
- Infinite data structures (rare in practice)
- Conditional evaluation without waste (`mkIf` only evaluates the branch that's needed)

**But also causes:** Confusing error messages when evaluation is forced in unexpected order, and hard-to-debug infinite recursion.

---

## 3. Style & Formatting

### Formatter

**nixfmt** (RFC 166) — the official Nix formatter, adopted by the NixOS project in 2024.

```bash
# Format a file
nixfmt file.nix

# Check without modifying
nixfmt --check file.nix
```

Not alejandra. nixfmt is the official standard now.

### Naming Conventions

| Context | Convention | Example |
|---------|-----------|---------|
| Files | `kebab-case.nix` | `desktop-gnome.nix`, `aletheia-service.nix` |
| Attribute names | `camelCase` | `buildInputs`, `shellHook`, `defaultPackage` |
| NixOS options | `dot.separated.camelCase` | `services.aletheia.enable` |
| Variables | `camelCase` | `craneLib`, `rustToolchain` |
| Flake outputs | Follow schema exactly | `packages`, `nixosConfigurations`, `devShells` |

### Indentation

Two spaces. No tabs. Consistent with our Rust style.

### Comments

```nix
# Single-line comment

/* Multi-line comment
   spanning multiple lines */
```

Use comments to explain **why**, not what. Same philosophy as Rust standards.

### String Style

```nix
# Short strings: double quotes
"hello world"

# Multi-line strings: double single quotes (trims leading whitespace)
''
  export PATH=${lib.makeBinPath [ pkgs.git ]}:$PATH
  echo "ready"
''

# Always quote URLs (RFC 45)
"https://github.com/NixOS/nixpkgs"
# NOT: https://github.com/NixOS/nixpkgs  (deprecated bare URL syntax)
```

---

## 4. Flake Structure

### Anatomy

Every flake has three top-level attributes:

```nix
{
  description = "Aletheia — distributed cognition system";

  inputs = {
    # Dependencies (other flakes or non-flake sources)
  };

  outputs = { self, nixpkgs, ... }: {
    # What this flake produces
  };
}
```

- `description` — Simple string. No Nix evaluation allowed at top level.
- `inputs` — Flake references (other flakes, git repos, paths). Locked by `flake.lock`.
- `outputs` — A function from inputs to an attrset following the flake schema.

### Input Conventions

```nix
inputs = {
  # Pin nixpkgs to a specific branch
  nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  # Crane for Rust builds
  crane.url = "github:ipetkov/crane";

  # Force transitive deps to use OUR nixpkgs (critical for consistency)
  crane.inputs.nixpkgs.follows = "nixpkgs";

  # Non-flake inputs (raw source)
  some-source = {
    url = "github:owner/repo";
    flake = false;
  };
};
```

**Always use `.follows`** for transitive nixpkgs dependencies. Without it, different inputs can pull different nixpkgs versions, breaking the reproducibility guarantee.

### Output Schema

```nix
outputs = { self, nixpkgs, crane, ... }: {
  # System-specific outputs (per architecture)
  packages.x86_64-linux.default = ...;
  packages.x86_64-linux.aletheia = ...;
  packages.aarch64-linux.default = ...;

  devShells.x86_64-linux.default = ...;

  checks.x86_64-linux = { ... };

  # System-independent outputs
  nixosConfigurations.worker-node = ...;
  nixosConfigurations.desktop = ...;

  nixosModules.default = ...;

  overlays.default = ...;
};
```

### Multi-System Pattern

Don't repeat yourself per architecture. Use `lib.genAttrs` or a helper:

```nix
let
  supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
  forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
in {
  packages = forAllSystems (system:
    let
      pkgs = nixpkgs.legacyPackages.${system};
      craneLib = crane.mkLib pkgs;
    in {
      default = craneLib.buildPackage { src = craneLib.cleanCargoSource ./.; };
    }
  );
}
```

Or use `flake-utils.lib.eachDefaultSystem` — but understand what it does. It's just a helper that generates the per-system attrsets. Don't put system-independent outputs (modules, overlays) inside it.

### Lock File

`flake.lock` is auto-generated and pinpoints exact versions of all inputs. **Commit it.** It IS your reproducibility. Update intentionally:

```bash
nix flake update              # Update all inputs
nix flake update nixpkgs      # Update only nixpkgs (Nix ≥2.19)
nix flake lock --update-input nixpkgs  # Older syntax
```

---

## 5. Module Patterns

NixOS modules are the building blocks of system configuration. A module is a function that returns an attrset with `imports`, `options`, and `config`.

### Module Structure

```nix
{ config, lib, pkgs, ... }:

let
  cfg = config.services.aletheia;
in {
  # What this module imports
  imports = [];

  # Declare options for consumers to set
  options.services.aletheia = {
    enable = lib.mkEnableOption "Aletheia distributed cognition system";

    package = lib.mkPackageOption pkgs "aletheia" { };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/aletheia";
      description = "Directory for Aletheia instance data";
    };

    agents = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "syn" ];
      description = "Agent identities to activate";
    };
  };

  # Define values (only active when enabled)
  config = lib.mkIf cfg.enable {
    systemd.services.aletheia = {
      description = "Aletheia Runtime";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/aletheia serve";
        WorkingDirectory = cfg.dataDir;
        StateDirectory = "aletheia";
        DynamicUser = true;
        Restart = "on-failure";
      };
    };

    environment.systemPackages = [ cfg.package ];
  };
}
```

### Key Module Functions

| Function | Priority | Purpose |
|----------|----------|---------|
| `lib.mkDefault` | 1000 | Set default value (overridable by normal assignment at priority 100) |
| `lib.mkForce` | 50 | Force a value (overrides almost everything) |
| `lib.mkOverride N` | N | Set with specific priority (lower number = higher priority) |
| `lib.mkIf cond { ... }` | — | Conditional config. Only evaluates if `cond` is true. |
| `lib.mkMerge [ ... ]` | — | Merge multiple config fragments. Use inside `config =`. |
| `lib.mkEnableOption "desc"` | — | Shorthand for a boolean option with default `false`. |
| `lib.mkPackageOption pkgs "name" {}` | — | Package option with default from pkgs. |
| `lib.mkBefore content` | 500 | Place content before default in ordered merges (lists, strings). |
| `lib.mkAfter content` | 1500 | Place content after default in ordered merges. |

### Pattern: `cfg` alias

Always alias the relevant config subtree at the top of the module:

```nix
let cfg = config.services.aletheia; in { ... }
```

This avoids repeating `config.services.aletheia.enable` everywhere.

### Pattern: Conditional blocks with mkIf + mkMerge

```nix
config = lib.mkMerge [
  (lib.mkIf cfg.enable {
    # Base configuration when enabled
  })
  (lib.mkIf (cfg.enable && cfg.gpu) {
    # Additional configuration when GPU is enabled
  })
];
```

### Pattern: Module composition

Split config into logical files. Use `imports` to compose:

```
modules/
├── core/
│   ├── boot.nix
│   ├── networking.nix
│   └── users.nix
├── services/
│   ├── aletheia.nix
│   └── monitoring.nix
├── desktop/
│   ├── gnome.nix
│   └── fonts.nix
└── profiles/
    ├── server.nix      # imports core/ + services/
    └── workstation.nix  # imports core/ + services/ + desktop/
```

---

## 6. Derivation & Packaging

### Rust with Crane

Crane is our chosen Rust packaging framework for Nix. It splits builds into dependency and source phases for maximum caching.

```nix
let
  craneLib = crane.mkLib pkgs;

  # Filter source to only Rust-relevant files
  src = craneLib.cleanCargoSource ./.;

  # Common args shared between dep and full builds
  commonArgs = {
    inherit src;
    strictDeps = true;
    buildInputs = [ pkgs.openssl ];
    nativeBuildInputs = [ pkgs.pkg-config ];
  };

  # Build ONLY dependencies (cached aggressively)
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # Build the actual binary (reuses dep artifacts)
  aletheia = craneLib.buildPackage (commonArgs // {
    inherit cargoArtifacts;
  });
in {
  packages.default = aletheia;

  # Run clippy as a check
  checks.clippy = craneLib.cargoClippy (commonArgs // {
    inherit cargoArtifacts;
    cargoClippyExtraArgs = "--all-targets -- --deny warnings";
  });

  # Run tests as a check
  checks.tests = craneLib.cargoNextest (commonArgs // {
    inherit cargoArtifacts;
  });
}
```

**Why Crane over alternatives:**

| Option | Verdict | Reason |
|--------|---------|--------|
| `crane` | ✅ Use this | Two-phase build (deps → source), best caching, actively maintained, composable |
| `buildRustPackage` (nixpkgs) | ❌ Avoid | Single-phase, rebuilds deps on every source change, requires `cargoHash` |
| `naersk` | ❌ Avoid | Maintained but less composable than crane, smaller community |

### NixOS Configuration

```nix
nixosConfigurations.worker-node = nixpkgs.lib.nixosSystem {
  system = "x86_64-linux";
  specialArgs = { inherit inputs; };
  modules = [
    ./hosts/worker-node/configuration.nix
    ./modules/services/aletheia.nix
    {
      services.aletheia = {
        enable = true;
        agents = [ "syn" "demiurge" "syl" "akron" ];
      };
    }
  ];
};
```

### Development Shell

```nix
devShells.default = pkgs.mkShell {
  inputsFrom = [ aletheia ];  # Inherit build deps
  packages = with pkgs; [
    rust-analyzer
    cargo-nextest
    cargo-deny
    nixfmt
  ];
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
};
```

---

## 7. Anti-Patterns

### ❌ `rec { ... }` — Avoid recursive attrsets

```nix
# BAD: Easy to create infinite recursion by shadowing
rec {
  a = 1;
  b = a + 2;
}

# GOOD: Use let...in
let a = 1; in {
  a = a;
  b = a + 2;
}

# GOOD: Or explicit self-reference
let
  attrset = {
    a = 1;
    b = attrset.a + 2;
  };
in attrset
```

### ❌ `with` at file scope — Pollutes namespace

```nix
# BAD: Where does `curl` come from? Static analysis can't tell.
with pkgs;
{
  environment.systemPackages = [ curl jq git ];
}

# GOOD: Explicit prefixing
{
  environment.systemPackages = [ pkgs.curl pkgs.jq pkgs.git ];
}

# ALSO GOOD: Explicit with inherit
{
  environment.systemPackages = builtins.attrValues {
    inherit (pkgs) curl jq git;
  };
}

# ACCEPTABLE: Small scoped `with` in list context (pragmatic)
{
  environment.systemPackages = with pkgs; [ curl jq git ];
}
# We allow this in list context ONLY when the scope is obvious.
# Prefer explicit pkgs.X for anything non-trivial.
```

### ❌ Lookup paths (`<nixpkgs>`) — Non-reproducible

```nix
# BAD: Depends on $NIX_PATH environment variable
import <nixpkgs> {}

# GOOD: Pin via flake input
import nixpkgs { system = "x86_64-linux"; config = {}; overlays = []; }
```

### ❌ Unpinned `import <nixpkgs> {}` — Impure config

```nix
# BAD: System files can influence the result
import nixpkgs {}

# GOOD: Explicitly set config and overlays
import nixpkgs { config = {}; overlays = []; }
```

### ❌ Shallow merge surprise with `//`

```nix
# DANGEROUS: Nested attrset b is replaced entirely
{ a = 1; b = { x = 1; y = 2; }; } // { b = { z = 3; }; }
# => { a = 1; b = { z = 3; }; }  — x and y are GONE

# SAFE: Use recursiveUpdate for deep merges
lib.recursiveUpdate
  { a = 1; b = { x = 1; y = 2; }; }
  { b = { z = 3; }; }
# => { a = 1; b = { x = 1; y = 2; z = 3; }; }
```

### ❌ Bare URLs

```nix
# BAD (deprecated syntax)
inputs.nixpkgs.url = https://github.com/NixOS/nixpkgs;

# GOOD
inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
```

### ❌ FHS assumptions

NixOS does NOT follow the Filesystem Hierarchy Standard. There is no `/usr/bin/`, no global `/lib/`. Every package lives in `/nix/store/<hash>-<name>/` with explicit references to its dependencies. Random binaries from GitHub will not work without wrapping.

```nix
# To run a non-Nix binary, wrap it
pkgs.buildFHSEnv {
  name = "my-binary";
  targetPkgs = pkgs: [ pkgs.glibc pkgs.openssl ];
  runScript = ./my-binary;
}
```

### ❌ System-independent outputs inside `eachDefaultSystem`

```nix
# BAD: nixosModules ends up under a system key
flake-utils.lib.eachDefaultSystem (system: {
  packages.default = ...;
  nixosModules.default = ...; # Wrong! This becomes nixosModules.x86_64-linux.default
});

# GOOD: Merge system-specific and system-independent separately
flake-utils.lib.eachDefaultSystem (system: {
  packages.default = ...;
}) // {
  nixosModules.default = ...;  # Correctly at top level
}
```

---

## 8. Our Conventions

### Aletheia Flake Structure

```
aletheia/
├── flake.nix              # Single entry point
├── flake.lock             # Committed, version-controlled
├── nix/
│   ├── package.nix        # Crane build definition
│   ├── checks.nix         # Clippy, tests, formatting checks
│   ├── shell.nix          # Dev shell definition
│   └── modules/
│       ├── aletheia.nix   # NixOS service module
│       └── default.nix    # Module aggregator
├── hosts/
│   ├── worker-node/       # Server configuration
│   │   ├── configuration.nix
│   │   └── hardware-configuration.nix
│   ├── desktop/           # Workstation (GNOME)
│   │   ├── configuration.nix
│   │   └── hardware-configuration.nix
│   └── usb/               # Recovery/provisioning image
│       └── configuration.nix
└── profiles/
    ├── server.nix         # Server profile (no GUI)
    ├── workstation.nix    # Desktop profile (GNOME + dev tools)
    └── recovery.nix       # Minimal USB profile
```

### Rules

1. **One flake.** Everything flows from `flake.nix`. No channel-based config, no `NIX_PATH`.
2. **Commit `flake.lock`.** It IS reproducibility.
3. **`.follows` on all transitive nixpkgs.** No version divergence.
4. **`config = {}; overlays = [];`** when importing nixpkgs. No impure system state.
5. **Crane for Rust.** Two-phase build. Always split deps from source.
6. **nixfmt for formatting.** No debate. Run in CI.
7. **Explicit > implicit.** `pkgs.git` over `with pkgs; [ git ]`. `inherit (pkgs) git` when you need brevity.
8. **`let ... in` over `rec`.** Always.
9. **Module options under `services.aletheia.*`** for the service module.
10. **`specialArgs`** to pass flake inputs to modules. Not `_module.args`.
11. **Checks gate CI.** `nix flake check` must pass. Include clippy, tests, formatting.
12. **No lookup paths.** No `<nixpkgs>`. No `$NIX_PATH` dependencies.

### Aletheia-Specific Packaging Notes

From the [Nix integration plan](../planning/nix-integration.md):

- **Single static binary.** No sidecars, no external databases.
- **hf-hub model download is RUNTIME, not build-time.** Don't pre-fetch models in the derivation. The binary downloads them on first run via `hf-hub`.
- **CozoDB is vendored.** Feature-gated in mneme. The Nix build needs C/C++ toolchain for the CozoDB compilation.
- **TLS decision: rustls + ring.** Minimal C/asm (constant-time crypto only). Not aws-lc-rs (heavy C++ toolchain). Not RustCrypto (alpha, RSA timing vulnerability).
- **No ONNX, no RocksDB.** Much simpler than a typical ML project.
- **Instance data at `/var/lib/aletheia/`.** Standard FHS location for service state on NixOS.

---

## 9. Tooling

### Essential

| Tool | Purpose | Install |
|------|---------|---------|
| `nix` | Package manager + language evaluator | System-level |
| `nixfmt` | Official formatter | `nix run nixpkgs#nixfmt` |
| `nix repl` | Interactive REPL for testing expressions | Built into `nix` |
| `nix eval` | Evaluate a Nix expression from file | Built into `nix` |
| `nix flake check` | Validate flake schema + run checks | Built into `nix` |
| `nix flake show` | Display flake outputs | Built into `nix` |
| `nixd` or `nil` | LSP for Nix (editor integration) | `nix profile install nixpkgs#nixd` |
| `statix` | Nix linter (catches anti-patterns) | `nix run nixpkgs#statix` |
| `deadnix` | Find unused code in Nix files | `nix run nixpkgs#deadnix` |
| `nix-tree` | Visualize dependency tree | `nix run nixpkgs#nix-tree` |

### Debugging

```bash
# REPL — test expressions interactively
nix repl -f .                  # Load current flake
nix-repl> :e lib.mkDefault     # View source of a function
nix-repl> :t someExpr          # Show type

# Trace — print-debug during evaluation
lib.traceVal someValue         # Prints value, returns it
lib.traceValSeq someValue      # Deep-evaluates before printing

# Instantiate without building — check evaluation
nix-instantiate -A nixosSystem.system

# Why is this dependency pulled in?
nix why-depends ./result /nix/store/<hash>-foo
```

---

## 10. Reference

### Useful Builtins

| Function | Purpose |
|----------|---------|
| `builtins.map f list` | Apply f to each element |
| `builtins.filter f list` | Keep elements where f returns true |
| `builtins.attrNames set` | List of attribute names (sorted) |
| `builtins.attrValues set` | List of attribute values (sorted by name) |
| `builtins.hasAttr name set` | Check if attrset has key |
| `builtins.readFile path` | Read file contents as string |
| `builtins.fromJSON str` | Parse JSON string to Nix value |
| `builtins.fromTOML str` | Parse TOML string to Nix value |
| `builtins.toJSON value` | Serialize Nix value to JSON string |
| `builtins.path { path; name; }` | Create store path with fixed name (reproducible) |
| `builtins.fetchGit { url; rev; }` | Fetch git repo |
| `builtins.foldl' f init list` | Left fold over list |
| `builtins.toString x` | Convert to string (wider than interpolation) |
| `builtins.typeOf x` | Returns type name as string |

### Useful lib Functions

| Function | Purpose |
|----------|---------|
| `lib.mkIf cond value` | Conditional config block |
| `lib.mkMerge [ ... ]` | Merge multiple config fragments |
| `lib.mkDefault value` | Set default (priority 1000) |
| `lib.mkForce value` | Force value (priority 50) |
| `lib.mkEnableOption "desc"` | Boolean option defaulting to false |
| `lib.mkPackageOption pkgs "name" {}` | Package option with default |
| `lib.genAttrs names f` | Generate attrset from list of names |
| `lib.filterAttrs f set` | Filter attrset by predicate |
| `lib.mapAttrs f set` | Map over attrset values |
| `lib.recursiveUpdate a b` | Deep merge of attrsets |
| `lib.flatten list` | Flatten nested lists |
| `lib.concatStringsSep sep list` | Join strings |
| `lib.makeBinPath paths` | Create PATH-style string |
| `lib.getExe pkg` | Get main executable path |
| `lib.hasPrefix prefix str` | String prefix check |
| `lib.optionals cond list` | Return list if cond, else [] |
| `lib.optional cond value` | Return [value] if cond, else [] |
| `lib.traceVal x` | Print value during eval (debug) |
| `lib.traceValSeq x` | Deep-eval then print (debug) |
| `lib.fix f` | Fixed-point combinator (self-referencing values) |

### Learning Path

1. **Nix language:** [ayats.org/blog/nix-tuto-1](https://ayats.org/blog/nix-tuto-1) — best single tutorial
2. **Derivations:** [ayats.org/blog/nix-tuto-2](https://ayats.org/blog/nix-tuto-2)
3. **Flakes:** [Practical Nix flake anatomy](https://vtimofeenko.com/posts/practical-nix-flake-anatomy-a-guided-tour-of-flake.nix/)
4. **NixOS modules:** [NixOS Modules Explained](https://saylesss88.github.io/NixOS_Modules_Explained_3.html)
5. **Full book:** [NixOS & Flakes Book](https://nixos-and-flakes.thiscute.world/)
6. **Crane (Rust packaging):** [crane.dev](https://crane.dev/)
7. **Official reference:** [nix.dev](https://nix.dev/)
8. **Option search:** [search.nixos.org/options](https://search.nixos.org/options)
9. **Function search:** [noogle.dev](https://noogle.dev/)
10. **Anti-patterns:** [nix.dev/anti-patterns](https://nix.dev/anti-patterns/language)

---

*Created: 2026-03-11. Peer to `standards/RUST.md`.*
