{
  description = "Login manager and screenlock library for Rust frontends";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        guiRuntimeLibs = with pkgs; [
          libGL
          libxkbcommon
          vulkan-loader
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
        ];

        simpleLock = pkgs.rustPlatform.buildRustPackage {
          pname = "limes-simple-lock";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "limes-simple-lock" ];
          cargoTestFlags = [ "-p" "limes-simple-lock" ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.pam ] ++ guiRuntimeLibs;
        };
      in
      {
        packages = {
          default = simpleLock;
          simple-lock = simpleLock;
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            pam
            pkg-config
            rust-analyzer
            rustc
            rustfmt
          ] ++ guiRuntimeLibs;

          env = {
            RUST_BACKTRACE = "1";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath guiRuntimeLibs;
          };
        };
      }
    );
}
