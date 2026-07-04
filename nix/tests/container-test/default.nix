# Largely derived from https://github.com/NixOS/nix/blob/14f7dae3e4eb0c34192d0077383a7f2a2d630129/tests/installer/default.nix
{ forSystem }:

let
  images = {

    # Found via https://hub.docker.com/_/ubuntu/ under "How is the rootfs build?"
    # Jammy
    "ubuntu-v22_04" = {
      tarball = builtins.fetchurl {
        url = "http://cdimage.ubuntu.com/ubuntu-base/releases/22.04/release/ubuntu-base-22.04-base-amd64.tar.gz";
        sha256 = "01sbpjb32x1z1yr9q78zrk0a6kfw5c4fxw1jqmm23g8ixryffvyz";
      };
      tester = ./default/Dockerfile;
      system = "x86_64-linux";
    };

    # Noble
    "ubuntu-v24_04" = {
      tarball = builtins.fetchurl {
        url = "http://cdimage.ubuntu.com/ubuntu-base/releases/24.04/release/ubuntu-base-24.04.3-base-amd64.tar.gz";
        sha256 = "1ybl31qj4ixyxi89h80gh71mpllnkqklbyj6pfrqil0ajgiwvhkb";
      };
      tester = ./default/Dockerfile;
      system = "x86_64-linux";
    };

    # Fedora 42 WSL rootfs – used for the bootc planner container test
    # (no systemd) reproducing
    # https://github.com/NixOS/nix-installer/issues/155
    # Re-compressed from .tar.xz to .tar.zst because podman import is
    # extremely slow at xz decompression inside a VM.
    "fedora-v42-bootc" =
      let
        xzTarball = builtins.fetchurl {
          url = "https://dl.fedoraproject.org/pub/fedora/linux/releases/42/Container/x86_64/images/Fedora-WSL-Base-42-1.1.x86_64.tar.xz";
          sha256 = "138vibdf0qcln3r0f116qvmq5vx8im9cy0xv2ml7r8ccsw2kvywr";
        };
        pkgs = forSystem "x86_64-linux" ({ pkgs, ... }: pkgs);
      in
      {
        tarball =
          pkgs.runCommand "fedora-42-rootfs.tar.zst"
            {
              nativeBuildInputs = with pkgs; [
                xz
                zstd
              ];
            }
            ''
              xz -dc ${xzTarball} | zstd -o $out
            '';
        tester = ./bootc/Dockerfile;
        system = "x86_64-linux";
      };
  };

  makeTest =
    containerTool: imageName:
    let
      image = images.${imageName};
    in
    with (forSystem image.system (
      {
        system,
        pkgs,
        lib,
        ...
      }:
      pkgs
    ));
    testers.nixosTest {
      name = "container-test-${imageName}";
      nodes = {
        machine =
          { config, pkgs, ... }:
          {
            virtualisation.${containerTool}.enable = true;
            virtualisation.diskSize = 4 * 1024;
          };
      };
      testScript = ''
        machine.start()
        machine.copy_from_host("${image.tarball}", "/image")
        machine.succeed("mkdir -p /test")
        machine.copy_from_host("${image.tester}", "/test/Dockerfile")
        machine.copy_from_host("${nix-installer-static}", "/test/nix-installer")
        machine.succeed("${containerTool} import /image default")
        machine.succeed("${containerTool} build -t test /test")
      '';
    };

  container-tests = builtins.mapAttrs (
    imageName: image:
    (with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs)); {
      ${image.system} = rec {
        docker = makeTest "docker" imageName;
        podman = makeTest "podman" imageName;
        all = pkgs.releaseTools.aggregate {
          name = "all";
          constituents = [
            docker
            podman
          ];
        };
      };
    })
  ) images;

in
container-tests
// {
  all."x86_64-linux" = rec {
    all = (
      with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
      pkgs.releaseTools.aggregate {
        name = "all";
        constituents = [
          docker
          podman
        ];
      }
    );
    docker = (
      with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
      pkgs.releaseTools.aggregate {
        name = "all";
        constituents = pkgs.lib.mapAttrsToList (name: value: value."x86_64-linux".docker) container-tests;
      }
    );
    podman = (
      with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
      pkgs.releaseTools.aggregate {
        name = "all";
        constituents = pkgs.lib.mapAttrsToList (name: value: value."x86_64-linux".podman) container-tests;
      }
    );
  };
}
