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

        rustPackage = packageName: binName: pkgs.rustPlatform.buildRustPackage {
          pname = binName;
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" packageName ];
          cargoTestFlags = [ "-p" packageName ];
          buildInputs = [ pkgs.pam ];
        };
      in
      {
        packages = {
          default = rustPackage "limes-cli" "limes";
          limes = rustPackage "limes-cli" "limes";
          frontend-native = rustPackage "limes-frontend-native" "limes-frontend-native";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            pam
            rust-analyzer
            rustc
            rustfmt
          ];

          env = {
            RUST_BACKTRACE = "1";
          };
        };
      }
    );
}
