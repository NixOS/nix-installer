{
  description = "Experimental Nix Installer";

  inputs = {
    nixpkgs.follows = "nix/nixpkgs";

    crane.url = "github:ipetkov/crane/v0.20.0";

    nix.url = "github:NixOS/nix/2.33.1";

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
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: (forSystem system f));

      forSystem = system: f: f rec {
        inherit system;
        pkgs = import nixpkgs { inherit system; overlays = [ self.overlays.default ]; };
        lib = pkgs.lib;
      };

      # Build the nix binary tarball and recompress with zstd
      # This is similar to nix's packaging/binary-tarball.nix but outputs zstd
      nixTarballZstd = { pkgs, system }:
        let
          nixPkg = nix.packages.${system}.nix;
          cacertPkg = pkgs.cacert;
          installerClosureInfo = pkgs.buildPackages.closureInfo {
            rootPaths = [ nixPkg cacertPkg ];
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
          } ''
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

      installerPackage = { pkgs, stdenv, buildPackages, extraRustFlags ? "" }:
        let
          craneLib = crane.mkLib pkgs;
          tarballPkg = nixTarballZstd { inherit pkgs; system = stdenv.hostPlatform.system; };
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
            };
          };
        in
        craneLib.buildPackage (sharedAttrs // {
          cargoArtifacts = craneLib.buildDepsOnly sharedAttrs;
          env = sharedAttrs.env // {
            RUSTFLAGS = "${if extraRustFlags != "" then " ${extraRustFlags}" else ""}";
            # Path to the embedded tarball
            NIX_TARBALL_PATH = "${tarballPkg}/nix.tar.zst";
            # Store paths known at compile time (no IFD - these come from passthru)
            NIX_STORE_PATH = nixStorePath;
            NSS_CACERT_STORE_PATH = cacertStorePath;
            NIX_VERSION = nixVersion;
          };
          postInstall = ''
            cp nix-installer.sh $out/bin/nix-installer.sh
          '';
        });
    in
    {
      overlays.default = final: prev:
        {
          nix-installer = installerPackage {
            pkgs = final;
            stdenv = final.stdenv;
            buildPackages = final.buildPackages;
          };

          nix-installer-static = final.pkgsStatic.callPackage installerPackage { };
        };


      devShells = forAllSystems ({ system, pkgs, ... }:
        let
          check = import ./nix/check.nix { inherit pkgs; };
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
          # Extract version from Cargo.toml
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          installerVersion = cargoToml.package.version;
          nixVersion = nix.packages.${pkgs.system}.nix.version;
          # Extract major.minor from both versions
          versionParts = ver: builtins.match "([0-9]+)\\.([0-9]+).*" ver;
          installerMajorMinor = versionParts installerVersion;
          nixMajorMinor = versionParts nixVersion;
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
          check-version-consistency =
            assert installerMajorMinor == nixMajorMinor ||
              throw "Version mismatch: installer version ${installerVersion} (${builtins.elemAt installerMajorMinor 0}.${builtins.elemAt installerMajorMinor 1}) does not match Nix version ${nixVersion} (${builtins.elemAt nixMajorMinor 0}.${builtins.elemAt nixMajorMinor 1}). The installer's major.minor must match the Nix version.";
            pkgs.runCommand "check-version-consistency" { } ''
              echo "Version consistency check passed: installer ${installerVersion} matches Nix ${nixVersion}"
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
