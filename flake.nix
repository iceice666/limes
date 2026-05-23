{
  description = "Log In Manager & Screenlock";

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

        rustPackage = packageName: binName: pkgs.rustPlatform.buildRustPackage {
          pname = binName;
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" packageName ];
          cargoTestFlags = [ "-p" packageName ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.pam ] ++ pkgs.lib.optionals (pkgs.lib.hasPrefix "limes-frontend-" packageName) guiRuntimeLibs;
        };
      in
      {
        packages = {
          default = rustPackage "limes-cli" "limes";
          limes = rustPackage "limes-cli" "limes";
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
