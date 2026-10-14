# Largely derived from https://github.com/NixOS/nix/blob/14f7dae3e4eb0c34192d0077383a7f2a2d630129/tests/installer/default.nix
{ lib, nixpkgsFor }:

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
  };

  makeTest =
    containerTool: imageName:
    let
      image = images.${imageName};
      pkgs = nixpkgsFor.${image.system};
    in
    pkgs.testers.nixosTest {
      name = "container-test-${imageName}";
      nodes = {
        machine = {
          virtualisation.${containerTool}.enable = true;
          virtualisation.diskSize = 4 * 1024;
        };
      };
      testScript = ''
        machine.start()
        machine.copy_from_host("${image.tarball}", "/image")
        machine.succeed("mkdir -p /test")
        machine.copy_from_host("${image.tester}", "/test/Dockerfile")
        # TODO: Stop grabbing this from the overlay.
        machine.copy_from_host("${pkgs.nix-installer-static}", "/test/nix-installer")
        machine.succeed("${containerTool} import /image default")
        machine.succeed("${containerTool} build -t test /test")
      '';
    };

  container-tests = builtins.mapAttrs (
    imageName: image:
    let
      pkgs = nixpkgsFor.${image.system};
    in
    {
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
    }
  ) images;

in
container-tests
// {
  all."x86_64-linux" = rec {
    all = (
      nixpkgsFor.x86_64-linux.releaseTools.aggregate {
        name = "all";
        constituents = [
          docker
          podman
        ];
      }
    );
    docker = (
      nixpkgsFor.x86_64-linux.releaseTools.aggregate {
        name = "all";
        constituents = lib.mapAttrsToList (name: value: value."x86_64-linux".docker) container-tests;
      }
    );
    podman = (
      nixpkgsFor.x86_64-linux.releaseTools.aggregate {
        name = "all";
        constituents = lib.mapAttrsToList (name: value: value."x86_64-linux".podman) container-tests;
      }
    );
  };
}
