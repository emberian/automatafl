{
  description = "AutomataFL workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };
        toolchain = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
          toolchain.default.override {
            targets = [ "wasm32-unknown-unknown" ];
          }
        );
        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
        src = craneLib.cleanCargoSource (craneLib.path ./.);

        commonArgs = {
          inherit src;
          cargoLock = ./Cargo.lock;
        };

        workspaceArgs = commonArgs // {
          pname = "automatafl-workspace";
        };
        workspaceArtifacts = craneLib.buildDepsOnly workspaceArgs;

        backendArgs = commonArgs // {
          pname = "automatafl-backend";
          cargoExtraArgs = "--package automatafl-backend --bin automatafl-backend";
        };
        backendArtifacts = craneLib.buildDepsOnly backendArgs;
        backendPackage = craneLib.buildPackage (backendArgs // {
          cargoArtifacts = backendArtifacts;
        });

        clientArgs = commonArgs // {
          pname = "automatafl-client";
          cargoExtraArgs = "--package automatafl-backend-client --bin automatafl-client";
        };
        clientArtifacts = craneLib.buildDepsOnly clientArgs;
        clientPackage = craneLib.buildPackage (clientArgs // {
          cargoArtifacts = clientArtifacts;
        });

        webappArgs = commonArgs // {
          pname = "automatafl-webapp";
          cargoExtraArgs = "--package automatafl-webapp";
          CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
        };
        webappArtifacts = craneLib.buildDepsOnly webappArgs;
        webappPackage = craneLib.buildPackage (webappArgs // {
          cargoArtifacts = webappArtifacts;
        });

        webappDist = pkgs.stdenv.mkDerivation {
          pname = "automatafl-webapp-dist";
          version = "0.1.0";
          inherit src;
          nativeBuildInputs = [
            toolchain
            pkgs.trunk
            pkgs.wasm-bindgen-cli
            pkgs.binaryen
          ];
          buildPhase = ''
            export HOME=$TMPDIR/home
            mkdir -p "$HOME"
            export CARGO_HOME=${webappArtifacts}/cargo-home
            export CARGO_TARGET_DIR=$TMPDIR/target
            cd webapp
            trunk build --release --frozen
          '';
          installPhase = ''
            mkdir -p $out
            cp -r dist/. $out/
          '';
        };

        integrationCheck = craneLib.cargoTest (workspaceArgs // {
          pname = "automatafl-integration";
          cargoExtraArgs = "--package automatafl-backend-client --test integration_test -- --test-threads=1";
          cargoArtifacts = workspaceArtifacts;
          nativeBuildInputs = with pkgs; [
            curl
            gnugrep
            gawk
            findutils
            coreutils
          ];
          checkPhase = ''
            runHook preCheck
            export HOME=$TMPDIR/home
            mkdir -p "$HOME/.config/automatafl-client"
            export CARGO_HOME=${workspaceArtifacts}/cargo-home
            export CARGO_TARGET_DIR=$TMPDIR/target
            export DATABASE_URL="surrealkv://$TMPDIR/automatafl.db"
            export BIND_ADDRESS="127.0.0.1:3000"
            backend_bin=${backendPackage}/bin/automatafl-backend
            $backend_bin &
            backend_pid=$!

            cleanup() {
              kill $backend_pid 2>/dev/null || true
            }
            trap cleanup EXIT

            ready=0
            for _ in $(seq 1 30); do
              if curl -sf "http://127.0.0.1:3000/api/health" >/dev/null 2>&1; then
                ready=1
                break
              fi
              sleep 1
            done

            if [ "$ready" -ne 1 ]; then
              echo "Backend failed to start" >&2
              exit 1
            fi

            cargo test --package automatafl-backend-client --test integration_test -- --test-threads=1
            AUTOMATAFL_CLIENT_BIN=${clientPackage}/bin/automatafl-client \
              bash backend-client/tests/cli_integration_test.sh
            runHook postCheck
          '';
        });
      in
      {
        packages = {
          default = backendPackage;
          automatafl-backend = backendPackage;
          automatafl-client = clientPackage;
          automatafl-webapp = webappPackage;
          automatafl-webapp-dist = webappDist;
        };

        checks = {
          fmt = craneLib.cargoFmt (workspaceArgs // { pname = "automatafl-fmt"; });
          clippy = craneLib.cargoClippy (workspaceArgs // {
            pname = "automatafl-clippy";
            cargoArtifacts = workspaceArtifacts;
            cargoExtraArgs = "--workspace --all-targets --exclude automatafl-webapp";
          });
          test = craneLib.cargoTest (workspaceArgs // {
            pname = "automatafl-tests";
            cargoArtifacts = workspaceArtifacts;
            cargoExtraArgs = "--workspace --exclude automatafl-webapp --exclude automatafl-backend-client";
          });
          integration = integrationCheck;
        };

        devShells.default = pkgs.mkShell {
          packages = [
            toolchain
            pkgs.rust-analyzer
            pkgs.pkg-config
            pkgs.trunk
            pkgs.wasm-bindgen-cli
            pkgs.binaryen
          ];
        };
      }
    );
}
