{
  description = "Login manager and screenlock libraries for Rust frontends";

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

        mkExample = pname: pkgs.rustPlatform.buildRustPackage {
          inherit pname;
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" pname ];
          cargoTestFlags = [ "-p" pname ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.pam ] ++ guiRuntimeLibs;
        };

        simpleLock = mkExample "limes-simple-lock";
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
