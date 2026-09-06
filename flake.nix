{
  description = "Aletheia — distributed cognition system";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, crane, ... }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs supportedSystems;
      rustVersion = "1.94.0";
      proskenionManifest = builtins.fromTOML (
        builtins.readFile ./crates/theatron/proskenion/Cargo.toml
      );
      proskenionPackage = proskenionManifest.package;
      proskenionName = proskenionPackage.name;
      proskenionVersion = proskenionPackage.version;
      proskenionCargoArgs = "--manifest-path crates/theatron/proskenion/Cargo.toml -p ${proskenionName}";

      perSystem =
        system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs {
            inherit system overlays;
            config = { };
          };

          rustToolchain = pkgs.rust-bin.stable.${rustVersion}.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
            ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # WHY(aletheia#7204): proskenion is a GTK3/webkit2gtk desktop app
          # (dioxus "desktop" feature -> wry -> gtk-rs + webkit2gtk-rs +
          # soup3), not a wgpu/Vulkan renderer — Cargo.lock carries no
          # wgpu/vulkan/wayland crate at all. This list must mirror the apt
          # packages `.github/workflows/desktop.yml` installs for desktop CI
          # (libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev librsvg2-dev); see
          # also docs/DESKTOP.md.
          #
          # NOTE: docs/design/graph-3d.md specs a *planned* wgpu-rendered 3D
          # graph view that does not exist in this crate yet (no wgpu/vulkan
          # crate in Cargo.lock). That is almost certainly why a wgpu/Vulkan
          # stack ended up here originally. If/when that feature actually
          # lands (Cargo.lock gains a wgpu dependency), add its native deps
          # back here rather than assuming this list is stale.
          gtkWebkitNativeDeps = [
            pkgs.gtk3 # libgtk-3-dev / gtk3-devel
            pkgs.webkitgtk_4_1 # libwebkit2gtk-4.1-dev / webkit2gtk4.1-devel — also provides javascriptcoregtk-4.1 and libsoup-3.0
            pkgs.xdotool # libxdo-dev / libxdo-devel — libxdo-sys links `-lxdo` for muda's global-shortcut/menu-accelerator support
            pkgs.librsvg # librsvg2-dev / librsvg2-devel — gdk-pixbuf SVG loader for the app icon/assets
          ];

          # WHY: no crate in Cargo.lock depends on `bindgen` or `cmake` — the
          # gtk-rs/webkit2gtk-rs sys crates resolve their C libraries through
          # `pkg-config`/`system-deps` only, so pulling in a bindgen/clang
          # toolchain here would just be dead weight in the build closure.
          nativeBuildDeps = [
            pkgs.pkg-config
            pkgs.pandoc
          ];

          src = pkgs.lib.cleanSourceWith {
            src = craneLib.path ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || builtins.match ".*/Dioxus\\.toml$" path != null
              || builtins.match ".*/assets/.*" path != null;
          };

          commonArgs = {
            inherit src;
            strictDeps = true;
            nativeBuildInputs = nativeBuildDeps;
            buildInputs = gtkWebkitNativeDeps;
            cargoExtraArgs = proskenionCargoArgs;
          };

          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              pname = "${proskenionName}-deps";
              version = proskenionVersion;
            }
          );

          proskenion = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = proskenionName;
              version = proskenionVersion;

              # WHY: the Nix build sandbox has no X11/Wayland display or
              # D-Bus session for GTK/WebKit to attach to. The build only
              # needs headers and link stubs to compile and link
              # successfully; running the app is the user's responsibility
              # outside the sandbox (see docs/DESKTOP.md).
              doCheck = false;
            }
          );

          proskenionShell = craneLib.devShell {
            inputsFrom = [ proskenion ];

            packages = [
              pkgs.dioxus-cli
              pkgs.cargo-deny
              pkgs.cargo-watch
              pkgs.pandoc
            ];

            # WHY: `cargo build`/`cargo run`/`dx serve` inside this shell
            # link against the GTK3/webkit2gtk shared libraries but, unlike
            # `nix build`'s output, are not run through nixpkgs' stdenv
            # fixup (no automatic RPATH patching), so the dynamic linker
            # needs them on LD_LIBRARY_PATH to resolve libgtk-3.so.0,
            # libwebkit2gtk-4.1.so, libsoup-3.0.so, etc. at runtime.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath gtkWebkitNativeDeps;
          };

          proskenionMetadataCheck = pkgs.runCommand "proskenion-flake-metadata" { } ''
            test "${proskenionName}" = "proskenion"
            test -n "${proskenionVersion}"
            touch "$out"
          '';
        in
        {
          packages = {
            inherit proskenion;
            default = proskenion;
          };

          devShells = {
            proskenion = proskenionShell;
            default = proskenionShell;
          };

          checks = {
            proskenion-flake-metadata = proskenionMetadataCheck;
          };
        };

      systemOutputs = forEachSystem perSystem;
    in
    {
      packages = nixpkgs.lib.mapAttrs (_: output: output.packages) systemOutputs;
      devShells = nixpkgs.lib.mapAttrs (_: output: output.devShells) systemOutputs;
      checks = nixpkgs.lib.mapAttrs (_: output: output.checks) systemOutputs;
    };
}
