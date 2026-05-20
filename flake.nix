{
  description = "Backend state and authentication library for limes";

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
        gstPlugins = with pkgs.gst_all_1; [
          gst-plugins-base
          gst-plugins-good
        ];
        gstPluginPath = pkgs.lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins;
        limesLockExample = pkgs.buildNpmPackage {
          pname = "limes-lock-example";
          version = "0.1.0";
          src = builtins.path { path = ./.; name = "source"; };
          sourceRoot = "source/examples/lock";
          npmDepsHash = "sha256-LgTH+veIqjPiZEuH7d9zHV2g2pAvVX/7trvpGCNE/bw=";
          installPhase = ''
            runHook preInstall
            mkdir -p $out
            cp -r dist/. $out/
            runHook postInstall
          '';
        };
        limesBackend = pkgs.rustPlatform.buildRustPackage {
          pname = "limes-backend";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "limes-backend" ];
          cargoTestFlags = [ "-p" "limes-backend" ];

          nativeBuildInputs = with pkgs; [
            llvmPackages.libclang
            pkg-config
          ];

          buildInputs = with pkgs; [
            atk
            cairo
            gdk-pixbuf
            glib
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
            gst_all_1.gst-plugins-good
            gtk3
            gtk-session-lock
            libsoup_3
            pam
            pango
            webkitgtk_4_1
          ];

          LIBCLANG_PATH = "${pkgs.lib.getLib pkgs.llvmPackages.libclang}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.pam}/include -I${pkgs.glibc.dev}/include";
        };
      in
      {
        packages = {
          default = limesBackend;
          lock-example = limesLockExample;
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            chromium
            clippy
            nodejs_24
            pkg-config
            rustc
            rustfmt
            rust-analyzer

            atk
            cairo
            gdk-pixbuf
            glib
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
            gst_all_1.gst-plugins-good
            gtk3
            gtk-session-lock
            gtk-session-lock.dev
            llvmPackages.libclang
            libsoup_3
            pam
            pango
            webkitgtk_4_1
          ];

          env = {
            RUST_BACKTRACE = "1";
            PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH = pkgs.lib.getExe pkgs.chromium;
            PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
            LIBCLANG_PATH = "${pkgs.lib.getLib pkgs.llvmPackages.libclang}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.pam}/include -I${pkgs.glibc.dev}/include";
            GST_PLUGIN_SYSTEM_PATH_1_0 = gstPluginPath;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.gtk-session-lock
              pkgs.llvmPackages.libclang
            ];
          };
        };
      }
    );
}
