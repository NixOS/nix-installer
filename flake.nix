{
  description = "Experimental Nix Installer";

  inputs = {
    # can track upstream versioning with
    # git show $most_recently_merged_commit:flake.lock | jq '.nodes[.nodes.root.inputs.nixpkgs].locked.rev'
    nixpkgs.url = "github:NixOS/nixpkgs/d98ce345cdab58477ca61855540999c86577d19d";

    crane.url = "github:ipetkov/crane/v0.20.0";

    nix = {
      url = "github:NixOS/nix/2.32.4";
      # Omitting `inputs.nixpkgs.follows = "nixpkgs";` on purpose
    };

    flake-compat.url = "github:edolstra/flake-compat/v1.0.0";
  };

  outputs =
    { self
    , nixpkgs
    , crane
    , nix
    , ...
    }:
    let
      nix_version = nix.packages.x86_64-linux.default.version;
      nix_tarball_url_prefix = "https://releases.nixos.org/nix/nix-${nix_version}/nix-${nix_version}-";
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: (forSystem system f));

      forSystem = system: f: f rec {
        inherit system;
        pkgs = import nixpkgs { inherit system; overlays = [ self.overlays.default ]; };
        lib = pkgs.lib;
      };

      installerPackage = { pkgs, stdenv, buildPackages, extraRustFlags ? "" }:
        let
          craneLib = crane.mkLib pkgs;
          sharedAttrs = {
            src = builtins.path {
              name = "nix-installer-source";
              path = self;
              filter = (path: type: baseNameOf path != "nix" && baseNameOf path != ".github");
            };

            # Required to link build scripts.
            depsBuildBuild = [ buildPackages.stdenv.cc ];

            env = {
              # For whatever reason, these don't seem to get set
              # automatically when using crane.
              #
              # Possibly related: <https://github.com/NixOS/nixpkgs/pull/369424>
              "CC_${stdenv.hostPlatform.rust.cargoEnvVarTarget}" = "${stdenv.cc.targetPrefix}cc";
              "CXX_${stdenv.hostPlatform.rust.cargoEnvVarTarget}" = "${stdenv.cc.targetPrefix}c++";
              "CARGO_TARGET_${stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER" = "${stdenv.cc.targetPrefix}cc";
              CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
            };
          };
        in
        craneLib.buildPackage (sharedAttrs // {
          cargoArtifacts = craneLib.buildDepsOnly sharedAttrs;
          env = sharedAttrs.env // {
            RUSTFLAGS = "--cfg tokio_unstable${if extraRustFlags != "" then " ${extraRustFlags}" else ""}";
            NIX_TARBALL_URL = "${nix_tarball_url_prefix}${stdenv.hostPlatform.system}.tar.xz";
          };
          postInstall = ''
            cp nix-installer.sh $out/bin/nix-installer.sh
          '';
        });
    in
    {
      overlays.default = final: prev:
        {
          # NOTE(cole-h): fixes build -- nixpkgs updated libsepol to 3.7 but didn't update
          # checkpolicy to 3.7, checkpolicy links against libsepol, and libsepol 3.7 changed
          # something in the API so checkpolicy 3.6 failed to build against libsepol 3.7
          # Can be removed once https://github.com/NixOS/nixpkgs/pull/335146 merges.
          checkpolicy = prev.checkpolicy.overrideAttrs ({ ... }: rec {
            version = "3.7";

            src = final.fetchurl {
              url = "https://github.com/SELinuxProject/selinux/releases/download/${version}/checkpolicy-${version}.tar.gz";
              sha256 = "sha256-/T4ZJUd9SZRtERaThmGvRMH4bw1oFGb9nwLqoGACoH8=";
            };
          });

          nix-installer = installerPackage {
            pkgs = final;
            stdenv = final.stdenv;
            buildPackages = final.buildPackages;
          };

          nix-installer-static = final.pkgsStatic.callPackage installerPackage { };
        };


      devShells = forAllSystems ({ pkgs, ... }:
        let
          check = import ./nix/check.nix { inherit pkgs; };
        in
        {
          default = pkgs.mkShell {
            name = "nix-install-shell";

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustcSrc}/library";
            NIX_TARBALL_URL = "${nix_tarball_url_prefix}${pkgs.stdenv.hostPlatform.system}.tar.xz";

            buildInputs = with pkgs; [
              rustc
              cargo
              clippy
              rustfmt
              shellcheck
              rust-analyzer
              cargo-outdated
              cacert
              # cargo-audit # NOTE(cole-h): build currently broken because of time dependency and Rust 1.80
              cargo-watch
              nixpkgs-fmt
              check.check-rustfmt
              check.check-spelling
              check.check-nixpkgs-fmt
              check.check-editorconfig
              check.check-semver
              check.check-clippy
              editorconfig-checker
              act
            ]
            ++ lib.optionals (pkgs.stdenv.isDarwin) (with pkgs; [
              libiconv
              darwin.apple_sdk.frameworks.Security
              darwin.apple_sdk.frameworks.SystemConfiguration
            ])
            ++ lib.optionals (pkgs.stdenv.isLinux) (with pkgs; [
              checkpolicy
              semodule-utils
              /* users are expected to have a system docker, too */
            ]);
          };
        });

      checks = forAllSystems ({ pkgs, ... }:
        let
          check = import ./nix/check.nix { inherit pkgs; };
        in
        {
          check-rustfmt = pkgs.runCommand "check-rustfmt" { buildInputs = [ check.check-rustfmt ]; } ''
            cd ${./.}
            check-rustfmt
            touch $out
          '';
          check-spelling = pkgs.runCommand "check-spelling" { buildInputs = [ check.check-spelling ]; } ''
            cd ${./.}
            check-spelling
            touch $out
          '';
          check-nixpkgs-fmt = pkgs.runCommand "check-nixpkgs-fmt" { buildInputs = [ check.check-nixpkgs-fmt ]; } ''
            cd ${./.}
            check-nixpkgs-fmt
            touch $out
          '';
          check-editorconfig = pkgs.runCommand "check-editorconfig" { buildInputs = [ pkgs.git check.check-editorconfig ]; } ''
            cd ${./.}
            check-editorconfig
            touch $out
          '';
        });

      packages = forAllSystems ({ system, pkgs, ... }:
        {
          inherit (pkgs) nix-installer;
        } // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          inherit (pkgs) nix-installer-static;
          default = pkgs.nix-installer-static;
        } // nixpkgs.lib.optionalAttrs (system == "aarch64-linux") {
          inherit (pkgs) nix-installer-static;
          default = pkgs.nix-installer-static;
        } // nixpkgs.lib.optionalAttrs (pkgs.stdenv.isDarwin) {
          default = pkgs.nix-installer;
        });

      apps = forAllSystems ({ pkgs, ... }: {
        test-action = {
          type = "app";
          program = toString (pkgs.writeShellScript "test-action" ''
            set -e
            echo "Testing GitHub Action with act..."
            ${pkgs.act}/bin/act -W .github/workflows/act-test.yml -j test-release --pull=false
          '');
        };
      });

      hydraJobs = {
        build = forAllSystems ({ system, pkgs, ... }: self.packages.${system}.default);
        # vm-test = import ./nix/tests/vm-test {
        #   inherit forSystem;
        #   inherit (nixpkgs) lib;

        #   binaryTarball = nix.tarballs_indirect;
        # };
        # container-test = import ./nix/tests/container-test {
        #   inherit forSystem;

        #   binaryTarball = nix.tarballs_indirect;
        # };
      };
    };
}
