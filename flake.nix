{
  description = "Nix flake for WOL Highlights extension development, cross-browser packaging, and AMO deployment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        webExt = pkgs.web-ext or pkgs.nodePackages.web-ext;

        deployApp = pkgs.writeShellApplication {
          name = "deploy-amo";
          runtimeInputs = [
            webExt
            pkgs.coreutils
            pkgs.nodejs
          ];
          text = ''
            for arg in "$@"; do
              if [ "$arg" = "--help" ] || [ "$arg" = "-h" ]; then
                exec web-ext sign --help
              fi
            done

            if [ -z "''${WEB_EXT_API_KEY:-}" ] || [ -z "''${WEB_EXT_API_SECRET:-}" ]; then
              echo "Error: Missing required Mozilla Add-ons (AMO) API credentials." >&2
              echo "Please set both WEB_EXT_API_KEY and WEB_EXT_API_SECRET in your environment:" >&2
              echo "  export WEB_EXT_API_KEY=\"your-amo-jwt-issuer\"" >&2
              echo "  export WEB_EXT_API_SECRET=\"your-amo-jwt-secret\"" >&2
              exit 1
            fi

            CHANNEL="''${AMO_CHANNEL:-listed}"
            echo "Deploying extension to Mozilla Add-ons (Channel: ''${CHANNEL})..."

            # Bake the shipped Google OAuth client into the package (no-op without creds).
            node scripts/gen-oauth.js --quiet

            exec web-ext sign \
              --api-key="''${WEB_EXT_API_KEY}" \
              --api-secret="''${WEB_EXT_API_SECRET}" \
              --channel="''${CHANNEL}" \
              --source-dir="wol-highlights" \
              --artifacts-dir="dist" \
              --ignore-files test "test/**" \
              "$@"
          '';
        };
      in
      {
        packages = {
          default = self.packages.${system}.wol;

          # `wol` — the pure-terminal OfflineWOL reader (wol-cli crate).
          # Only the CLI is built; the Tauri desktop app stays out of the closure.
          wol = pkgs.rustPlatform.buildRustPackage {
            pname = "wol";
            version = (nixpkgs.lib.importTOML ./Cargo.toml).workspace.package.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "wol-cli" ];
            cargoTestFlags = [ "-p" "wol-cli" ];
            doCheck = true;
            # buildRustPackage's default install expects the binary to be named
            # after `pname`; the crate ships `wol` from the `wol-cli` package.
            installPhase = ''
              runHook preInstall
              install -Dm755 "$(find target -maxdepth 3 -type f -name wol -perm -u+x | head -n 1)" "$out/bin/wol"
              runHook postInstall
            '';
            meta = {
              description = "OfflineWOL in your terminal: cached wol.jw.org reading with JW Library highlights";
              homepage = "https://github.com/danielmain/wol-highlights";
              license = nixpkgs.lib.licenses.mit;
              mainProgram = "wol";
              platforms = nixpkgs.lib.platforms.unix;
            };
          };
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.nodejs
            webExt
            pkgs.cargo
            pkgs.rustc
            pkgs.pkg-config
            pkgs.jq
            pkgs.zip
            pkgs.unzip
            pkgs.resvg
            pkgs.rbw
            pkgs.bitwarden-cli
          ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.atk
            pkgs.dbus
            pkgs.gdk-pixbuf
            pkgs.glib
            pkgs.glib-networking
            pkgs.gsettings-desktop-schemas
            pkgs.gtk3
            pkgs.libayatana-appindicator
            pkgs.librsvg
            pkgs.libsoup_3
            pkgs.openssl
            pkgs.pango
            pkgs.webkitgtk_4_1
          ];
        };

        apps = rec {
          deploy = flake-utils.lib.mkApp {
            drv = deployApp;
          };
          wol = flake-utils.lib.mkApp {
            drv = self.packages.${system}.wol;
          };
          default = deploy;
        };
      }
    );
}
