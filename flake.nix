{
  description = "Aletheia — distributed cognition system";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane }:
    let
      system = "x86_64-linux";
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };

      rustVersion = "1.94.0";
      rustToolchain = pkgs.rust-bin.stable.${rustVersion}.default.override {
        extensions = [ "rust-src" "rust-analyzer" ];
      };

      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      # Native libraries required by WGPU/Blitz at build and runtime.
      wgpuNativeDeps = with pkgs; [
        vulkan-loader
        libxkbcommon
        wayland

        # X11
        xorg.libX11
        xorg.libXcursor
        xorg.libXrandr
        xorg.libXi
        xorg.libxcb

        # Font rendering
        fontconfig
        freetype
      ];

      # Build-time tools needed by native crates (C compilation, linking).
      nativeBuildDeps = with pkgs; [
        pkg-config
        cmake
        rustPlatform.bindgenHook
      ];

      # Shared source filter: include Rust sources, Cargo manifests,
      # Dioxus config, and any asset files.
      src = pkgs.lib.cleanSourceWith {
        src = craneLib.path ./.;
        filter = path: type:
          (craneLib.filterCargoSources path type)
          || builtins.match ".*Dioxus\\.toml$" path != null
          || builtins.match ".*\\.css$" path != null
          || builtins.match ".*\\.html$" path != null;
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        nativeBuildInputs = nativeBuildDeps;
        buildInputs = wgpuNativeDeps;
      };

      # Build workspace dependencies (cached separately from source).
      cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
        pname = "aletheia-workspace";
        version = "0.11.0";
      });

      aletheia-desktop = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "aletheia-desktop";
        version = "0.11.0";

        cargoExtraArgs = "-p theatron-desktop";

        # WHY: Nix sandbox has no GPU. The build only needs headers and
        # link stubs, not a running GPU. Runtime GPU access is the user's
        # responsibility.
        doCheck = false;
      });
    in
    {
      packages.${system} = {
        inherit aletheia-desktop;
        default = aletheia-desktop;
      };

      devShells.${system}.default = craneLib.devShell {
        # Inherit build inputs so native libraries are available.
        inputsFrom = [ aletheia-desktop ];

        packages = with pkgs; [
          # Dioxus CLI for hot-patching development
          dioxus-cli

          # Build tooling
          cargo-deny
          cargo-watch

          # Wayland session support
          wayland-protocols
          wayland-scanner
        ];

        # WHY: WGPU discovers the Vulkan ICD loader and GPU-adjacent
        # libraries via LD_LIBRARY_PATH at runtime. Without this, the
        # desktop app cannot find the Vulkan driver on NixOS.
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath wgpuNativeDeps;
      };

