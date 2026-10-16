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
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.nodejs
            webExt
            pkgs.jq
            pkgs.zip
            pkgs.unzip
            pkgs.resvg
            pkgs.rbw
            pkgs.bitwarden-cli
          ];
        };

        apps = rec {
          deploy = flake-utils.lib.mkApp {
            drv = deployApp;
          };
          default = deploy;
        };
      }
    );
}
