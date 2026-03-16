# NixOS build compatibility

NixOS-specific considerations for building and running Dioxus desktop apps.

---

## Blitz native renderer: broken on NixOS

**Status: hard blocker.** Dioxus issue #5133 — app renders a blank window on NixOS. The issue is open with no fix as of March 2026.

### Required dependencies (even though it doesn't render)

```nix
buildInputs = [
  pkg-config
  openssl
  python3
  wayland
  libxkbcommon
  vulkan-loader
];
```

### Runtime library path

```nix
RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath [
  pkgs.wayland
  pkgs.libxkbcommon
  pkgs.vulkan-loader
]}";
```

Even with correct library paths, the blank window persists. The root cause appears to be in wgpu/winit surface initialization on NixOS.

---

## Webview renderer: works with caveats

The wry webview renderer uses the system WebView (WebKitGTK on Linux). This works on NixOS.

### Required dependencies

```nix
buildInputs = with pkgs; [
  pkg-config
  openssl
  glib
  gtk3
  webkitgtk_4_1  # or webkitgtk_6_0 depending on wry version
  libsoup_3
  cairo
  pango
  gdk-pixbuf
  atk
];

# For development (dx serve)
nativeBuildInputs = with pkgs; [
  pkg-config
  rustPlatform.bindgenHook
];
```

### Known caveat

File input button (`<input type=file>`) may not open a native file picker dialog. Use `rfd` crate as a fallback for file selection.

---

## Flake template

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [ rust-overlay.overlays.default ];
    };
    rust = pkgs.rust-bin.stable.latest.default;
  in {
    devShells.${system}.default = pkgs.mkShell {
      nativeBuildInputs = with pkgs; [
        rust
        pkg-config
        rustPlatform.bindgenHook
      ];
      buildInputs = with pkgs; [
        openssl
        # Webview (wry) deps
        glib
        gtk3
        webkitgtk_4_1
        libsoup_3
        cairo
        pango
        gdk-pixbuf
        atk
      ];
    };
  };
}
```

---

## Recommendation

Use the **webview renderer** on NixOS. Blitz native is blocked by the blank-window bug (#5133). Include the Nix flake template above in the `theatron-desktop` crate when it moves to production.
