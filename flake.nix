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

          wgpuNativeDeps = [
            pkgs.vulkan-loader
            pkgs.libxkbcommon
            pkgs.wayland

            pkgs.xorg.libX11
            pkgs.xorg.libXcursor
            pkgs.xorg.libXrandr
            pkgs.xorg.libXi
            pkgs.xorg.libxcb

            pkgs.fontconfig
            pkgs.freetype
          ];

          nativeBuildDeps = [
            pkgs.pkg-config
            pkgs.cmake
            pkgs.rustPlatform.bindgenHook
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
            buildInputs = wgpuNativeDeps;
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

              # WHY: Nix sandbox has no GPU. The build only needs headers and
              # link stubs, not a running GPU. Runtime GPU access is the user's
              # responsibility.
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
              pkgs.wayland-protocols
              pkgs.wayland-scanner
            ];

            # WHY: WGPU discovers the Vulkan ICD loader and GPU-adjacent
            # libraries via LD_LIBRARY_PATH at runtime. Without this, the
            # desktop app cannot find the Vulkan driver on NixOS.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath wgpuNativeDeps;
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
