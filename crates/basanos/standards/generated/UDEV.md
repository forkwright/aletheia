# Udev Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [UDEV/attr-match-required](#udevattr-match-required)

## `UDEV/attr-match-required` {#udevattr-match-required}

- Severity: `warning`
- Scope: `project:ops`

udev rules that grant permissions or run programs must match on at least idVendor and idProduct (or equivalent specific attributes) in addition to SUBSYSTEM. Broad SUBSYSTEM-only matches fire on every device in the class, which can grant unintended permissions to unrelated hardware.

### Examples

**Good:** Match on both idVendor and idProduct so the rule fires on the correct device.

```text
SUBSYSTEM=="usb", ATTRS{idVendor}=="1234", ATTRS{idProduct}=="5678", MODE="0660", GROUP="plugdev"
```

**Bad:** Match on subsystem alone, which may fire on every USB device.

```text
SUBSYSTEM=="usb", MODE="0660"
```

