# Shell Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [SHELL/set-euo-pipefail](#shellset-euo-pipefail)

## `SHELL/set-euo-pipefail` {#shellset-euo-pipefail}

- Severity: `warning`
- Scope: `language:shell`

Every bash script in the fleet must open with `set -euo pipefail`. `-e` exits on the first error, `-u` treats unset variables as errors, and `-o pipefail` propagates failure through pipes. Without these options, errors in intermediate pipeline stages are silently swallowed and the script continues in an undefined state.

### Examples

**Good:** Open every shell script with set -euo pipefail.

```text
#!/usr/bin/env bash
set -euo pipefail

kanon gate
kanon land
```

**Bad:** Run a shell script without error options so failures are silently swallowed.

```text
#!/usr/bin/env bash
rm -rf /tmp/build  # if this fails, the script continues
```

