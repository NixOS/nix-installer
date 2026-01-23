{
  description = "Experimental Nix Installer";

  inputs = {
    nixpkgs.follows = "nix/nixpkgs";

    crane.url = "github:ipetkov/crane/v0.20.0";

    nix.url = "github:NixOS/nix/2.33.1";

    flake-compat.url = "github:edolstra/flake-compat/v1.0.0";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      nix,
      treefmt-nix,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: (forSystem system f));

      forSystem =
        system: f:
        f rec {
          inherit system;
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlays.default ];
          };
          lib = pkgs.lib;
        };

      # Eval the treefmt modules from ./treefmt.nix
      treefmtEval = forAllSystems ({ pkgs, ... }: treefmt-nix.lib.evalModule pkgs ./treefmt.nix);

      # Build the nix binary tarball and recompress with zstd
      # This is similar to nix's packaging/binary-tarball.nix but outputs zstd
      nixTarballZstd =
        { pkgs, system }:
        let
          nixPkg = nix.packages.${system}.nix;
          cacertPkg = pkgs.cacert;
          installerClosureInfo = pkgs.buildPackages.closureInfo {
            rootPaths = [
              nixPkg
              cacertPkg
            ];
          };
        in
        pkgs.runCommand "nix-tarball-zstd-${nixPkg.version}"
          {
            nativeBuildInputs = [ pkgs.zstd ];
            # Export these so they can be read without IFD
            passthru = {
              inherit nixPkg cacertPkg;
              nixStorePath = nixPkg.outPath;
              cacertStorePath = cacertPkg.outPath;
              nixVersion = nixPkg.version;
            };
          }
          ''
            mkdir -p $out

            dir=nix-${nixPkg.version}-${system}

            # Copy the reginfo (closure registration) to temp dir
            cp ${installerClosureInfo}/registration $TMPDIR/reginfo

            # Create tarball matching nix's binary-tarball.nix structure
            # Use --hard-dereference to convert symlinks to regular files
            # Use --transform to rewrite /nix/store paths to $dir/store
            tar cf - \
              --owner=0 --group=0 --mode=u+rw,uga+r \
              --mtime='1970-01-01' \
              --absolute-names \
              --hard-dereference \
              --transform "s,$TMPDIR/reginfo,$dir/.reginfo," \
              --transform "s,$NIX_STORE,$dir/store,S" \
              $TMPDIR/reginfo \
              $(cat ${installerClosureInfo}/store-paths) \
              | zstd -19 -T0 -o $out/nix.tar.zst
          '';

      # Shared crane build setup - returns { package, clippy, cargoArtifacts }
      mkCraneBuilds =
        {
          pkgs,
          stdenv,
          buildPackages,
          extraRustFlags ? "",
        }:
        let
          craneLib = crane.mkLib pkgs;
          tarballPkg = nixTarballZstd {
            inherit pkgs;
            system = stdenv.hostPlatform.system;
          };
          # Get paths directly from passthru - no IFD!
          nixStorePath = tarballPkg.passthru.nixStorePath;
          cacertStorePath = tarballPkg.passthru.cacertStorePath;
          nixVersion = tarballPkg.passthru.nixVersion;
          sharedAttrs = {
            src = builtins.path {
              name = "nix-installer-source";
              path = self;
              filter = (path: type: baseNameOf path != "nix" && baseNameOf path != ".github");
            };

            nativeBuildInputs = [ tarballPkg ];

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
              # Path to the embedded tarball
              NIX_TARBALL_PATH = "${tarballPkg}/nix.tar.zst";
              # Store paths known at compile time (no IFD - these come from passthru)
              NIX_STORE_PATH = nixStorePath;
              NSS_CACERT_STORE_PATH = cacertStorePath;
              NIX_VERSION = nixVersion;
            };
          };
          cargoArtifacts = craneLib.buildDepsOnly sharedAttrs;
        in
        {
          inherit cargoArtifacts;

          package = craneLib.buildPackage (
            sharedAttrs
            // {
              inherit cargoArtifacts;
              env = sharedAttrs.env // {
                RUSTFLAGS = "${if extraRustFlags != "" then " ${extraRustFlags}" else ""}";
              };
              postInstall = ''
                cp nix-installer.sh $out/bin/nix-installer.sh
              '';
            }
          );

          clippy = craneLib.cargoClippy (
            sharedAttrs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            }
          );
        };

      installerPackage =
        {
          pkgs,
          stdenv,
          buildPackages,
          extraRustFlags ? "",
        }:
        (mkCraneBuilds {
          inherit
            pkgs
            stdenv
            buildPackages
            extraRustFlags
            ;
        }).package;
    in
    {
      # for `nix fmt`
      formatter = forAllSystems ({ system, ... }: treefmtEval.${system}.config.build.wrapper);

      overlays.default = final: prev: {
        nix-installer = installerPackage {
          pkgs = final;
          stdenv = final.stdenv;
          buildPackages = final.buildPackages;
        };

        nix-installer-static = final.pkgsStatic.callPackage installerPackage { };
      };

      devShells = forAllSystems (
        { system, pkgs, ... }:
        let
          tarballPkg = nixTarballZstd { inherit pkgs system; };
        in
        {
          default = pkgs.mkShell {
            name = "nix-install-shell";

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustcSrc}/library";
            NIX_TARBALL_PATH = "${tarballPkg}/nix.tar.zst";
            NIX_STORE_PATH = tarballPkg.passthru.nixStorePath;
            NSS_CACERT_STORE_PATH = tarballPkg.passthru.cacertStorePath;
            NIX_VERSION = tarballPkg.passthru.nixVersion;

            buildInputs =
              with pkgs;
              [
                # Rust development
                rustc
                cargo
                clippy
                rust-analyzer
                cargo-outdated
                cargo-semver-checks
                # cargo-audit # NOTE(cole-h): build currently broken because of time dependency and Rust 1.80
                cargo-watch
                cacert

                # treefmt (for `nix fmt` and manual formatting)
                treefmtEval.${system}.config.build.wrapper

                # Testing
                act
              ]
              ++ lib.optionals (pkgs.stdenv.isDarwin) (
                with pkgs;
                [
                  libiconv
                  darwin.apple_sdk.frameworks.Security
                  darwin.apple_sdk.frameworks.SystemConfiguration
                ]
              )
              ++ lib.optionals (pkgs.stdenv.isLinux) (
                with pkgs;
                [
                  checkpolicy
                  semodule-utils
                  # users are expected to have a system docker, too
                ]
              );
          };
        }
      );

      checks = forAllSystems (
        { system, pkgs, ... }:
        let
          craneBuilds = mkCraneBuilds {
            inherit pkgs;
            stdenv = pkgs.stdenv;
            buildPackages = pkgs.buildPackages;
          };

          # Extract version from Cargo.toml
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          installerVersion = cargoToml.package.version;
          nixVersion = nix.packages.${system}.nix.version;
          # Extract major.minor from both versions
          versionParts = ver: builtins.match "([0-9]+)\\.([0-9]+).*" ver;
          installerMajorMinor = versionParts installerVersion;
          nixMajorMinor = versionParts nixVersion;
        in
        {
          # treefmt handles: rustfmt, nixfmt, shfmt, shellcheck, typos, taplo, yamlfmt, actionlint, editorconfig
          formatting = treefmtEval.${system}.config.build.check self;

          check-version-consistency =
            assert
              installerMajorMinor == nixMajorMinor
              || throw "Version mismatch: installer version ${installerVersion} (${builtins.elemAt installerMajorMinor 0}.${builtins.elemAt installerMajorMinor 1}) does not match Nix version ${nixVersion} (${builtins.elemAt nixMajorMinor 0}.${builtins.elemAt nixMajorMinor 1}). The installer's major.minor must match the Nix version.";
            pkgs.runCommand "check-version-consistency" { } ''
              echo "Version consistency check passed: installer ${installerVersion} matches Nix ${nixVersion}"
              touch $out
            '';

          inherit (craneBuilds) clippy;
        }
      );

      packages = forAllSystems (
        { system, pkgs, ... }:
        {
          inherit (pkgs) nix-installer;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          inherit (pkgs) nix-installer-static;
          default = pkgs.nix-installer-static;
        }
        // nixpkgs.lib.optionalAttrs (system == "aarch64-linux") {
          inherit (pkgs) nix-installer-static;
          default = pkgs.nix-installer-static;
        }
        // nixpkgs.lib.optionalAttrs (pkgs.stdenv.isDarwin) {
          default = pkgs.nix-installer;
        }
      );

      apps = forAllSystems (
        { pkgs, ... }:
        {
          test-action = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "test-action" ''
                set -e
                echo "Testing GitHub Action with act..."
                ${pkgs.act}/bin/act -W .github/workflows/act-test.yml -j test-release --pull=false
              ''
            );
          };
        }
      );

      hydraJobs = {
        build = forAllSystems ({ system, pkgs, ... }: self.packages.${system}.default);
        #vm-test = import ./nix/tests/vm-test {
        #  inherit forSystem;
        #  inherit (nixpkgs) lib;

        #  binaryTarball = nix.tarballs_indirect;
        #};
        #container-test = import ./nix/tests/container-test {
        #  inherit forSystem;

        #  binaryTarball = nix.tarballs_indirect;
        #};
      };
    };
}
