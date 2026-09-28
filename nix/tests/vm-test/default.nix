# Largely derived from https://github.com/NixOS/nix/blob/14f7dae3e4eb0c34192d0077383a7f2a2d630129/tests/installer/default.nix
{
  forSystem,
  lib,
}:

let
  nix-installer-install = ''
    RUST_BACKTRACE="full" ./nix-installer install --no-confirm --logger pretty --log-directive nix_installer=trace
  '';
  nix-installer-install-quiet = ''
    RUST_BACKTRACE="full" ./nix-installer install --no-confirm
  '';
  installCases = rec {
    install-default = {
      install = nix-installer-install;
      check = ''
        set -ex

        dir /nix
        dir /nix/store

        ls -lah /nix/var/nix/profiles/per-user
        ls -lah /nix/var/nix/daemon-socket

        if systemctl is-active nix-daemon.socket; then
          echo "nix-daemon.socket was active"
        else
          echo "nix-daemon.socket was not active, should be"
          exit 1
        fi
        if systemctl is-failed nix-daemon.socket; then
          echo "nix-daemon.socket is failed"
          sudo journalctl -eu nix-daemon.socket
          exit 1
        fi

        if !(sudo systemctl start nix-daemon.service); then
          echo "nix-daemon.service failed to start"
          sudo journalctl -eu nix-daemon.service
          exit 1
        fi

        if systemctl is-failed nix-daemon.service; then
          echo "nix-daemon.service is failed"
          sudo journalctl -eu nix-daemon.service
          exit 1
        fi

        if !(sudo systemctl stop nix-daemon.service); then
          echo "nix-daemon.service failed to stop"
          sudo journalctl -eu nix-daemon.service
          exit 1
        fi

        sudo -i nix store ping --store daemon
        nix store ping --store daemon

        sudo -i nix-env --version
        nix-env --version
        sudo -i nix --extra-experimental-features nix-command store ping
        nix --extra-experimental-features nix-command store ping

        out=$(nix-build --no-substitute -E 'derivation { name = "foo"; system = "x86_64-linux"; builder = "/bin/sh"; args = ["-c" "echo foobar > $out"]; }')
        [[ $(cat $out) = foobar ]]
      '';
      uninstall = ''
        /nix/nix-installer uninstall --no-confirm
      '';
      uninstallCheck = ''
        if which nix; then
          echo "nix existed on path after uninstall"
          exit 1
        fi

        for i in $(seq 1 32); do
          if id -u nixbld$i; then
            echo "User nixbld$i exists after uninstall"
            exit 1
          fi
        done
        if grep "^nixbld:" /etc/group; then
          echo "Group nixbld exists after uninstall"
          exit 1
        fi

        if sudo -i nix store ping --store daemon; then
          echo "Could run nix store ping after uninstall"
          exit 1
        fi

        if [ -d /nix/store ]; then
          echo "/nix/store exists after uninstall"
          exit 1
        fi
        if [ -d /nix/var ]; then
          echo "/nix/var exists after uninstall"
          exit 1
        fi

        if [ -d /etc/nix/nix.conf ]; then
          echo "/etc/nix/nix.conf exists after uninstall"
          exit 1
        fi

        if [ -f /etc/systemd/system/nix-daemon.socket ]; then
          echo "/etc/systemd/system/nix-daemon.socket exists after uninstall"
          exit 1
        fi

        if [ -f /etc/systemd/system/nix-daemon.service ]; then
          echo "/etc/systemd/system/nix-daemon.socket exists after uninstall"
          exit 1
        fi


        if systemctl status nix-daemon.socket > /dev/null; then
          echo "systemd unit nix-daemon.socket still exists after uninstall"
          exit 1
        fi

        if systemctl status nix-daemon.service > /dev/null; then
          echo "systemd unit nix-daemon.service still exists after uninstall"
          exit 1
        fi
      '';
    };
    install-no-start-daemon = {
      install = ''
        RUST_BACKTRACE="full" ./nix-installer install linux --no-confirm --logger pretty --log-directive nix_installer=info --no-start-daemon
      '';
      check = ''
        set -ex

        if systemctl is-active nix-daemon.socket; then
          echo "nix-daemon.socket was running, should not be"
          exit 1
        fi
        if systemctl is-active nix-daemon.service; then
          echo "nix-daemon.service was running, should not be"
          exit 1
        fi
        sudo systemctl start nix-daemon.socket

        nix-env --version
        nix --extra-experimental-features nix-command store ping
        out=$(nix-build --no-substitute -E 'derivation { name = "foo"; system = "x86_64-linux"; builder = "/bin/sh"; args = ["-c" "echo foobar > $out"]; }')

        [[ $(cat $out) = foobar ]]
      '';
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    install-daemonless = {
      install = ''
        RUST_BACKTRACE="full" ./nix-installer install linux --no-confirm --logger pretty --log-directive nix_installer=info --init none
      '';
      check = ''
        set -ex
        sudo -i nix-env --version
        sudo -i nix --extra-experimental-features nix-command store ping

        echo 'derivation { name = "foo"; system = "x86_64-linux"; builder = "/bin/sh"; args = ["-c" "echo foobar > $out"]; }' | sudo tee -a /drv
        out=$(sudo -i nix-build --no-substitute /drv)

        [[ $(cat $out) = foobar ]]
      '';
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    install-bind-mounted-nix = {
      preinstall = ''
        sudo mkdir -p /nix
        sudo mkdir -p /bind-mount-for-nix
        sudo mount --bind /bind-mount-for-nix /nix
      '';
      install = installCases.install-default.install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    install-invalid-custom-conf = {
      preinstall = ''
        sudo mkdir -p /etc/nix
        sudo touch /etc/nix/nix.custom.conf
        sudo chmod 777 /etc/nix/nix.custom.conf
        echo "foobar" > /etc/nix/nix.custom.conf
      '';
      install = installCases.install-default.install;
      check = installCases.install-default.check + ''
        grep --quiet "^# foobar" /etc/nix/nix.custom.conf
      '';
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    # On SUSE, the Nix snippet must go to /etc/bash.bashrc.local (not
    # /etc/bash.bashrc) to avoid PATH conflicts with SUSE's /etc/profile
    # sourcing in bash.bashrc for SSH sessions. On other distros,
    # /etc/bash.bashrc should have the snippet and .local must not exist.
    install-shell-profile-locations = {
      install = nix-installer-install;
      check = installCases.install-default.check + ''
        . /etc/os-release
        case "$ID" in
          sles|opensuse-*)
            grep -q "nix-daemon.sh" /etc/bash.bashrc.local
            if grep -q "nix-daemon.sh" /etc/bash.bashrc; then
              echo "/etc/bash.bashrc should not contain Nix snippet on SUSE"
              exit 1
            fi
            ;;
          arch)
            # On Arch, /etc/bash.bashrc has a non-interactive guard that makes
            # appended snippets dead code for SSH command mode. The installer
            # skips bash.bashrc and sets BASH_ENV in /etc/environment instead.
            grep -q "BASH_ENV.*nix-daemon.sh" /etc/environment
            if grep -q "nix-daemon.sh" /etc/bash.bashrc; then
              echo "/etc/bash.bashrc should not contain Nix snippet on Arch"
              exit 1
            fi
            ;;
          *)
            grep -q "nix-daemon.sh" /etc/bash.bashrc
            if [ -f /etc/bash.bashrc.local ] && grep -q "nix-daemon.sh" /etc/bash.bashrc.local; then
              echo "/etc/bash.bashrc.local should not exist on non-SUSE"
              exit 1
            fi
            ;;
        esac
      '';
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck + ''
        if [ -f /etc/bash.bashrc.local ] && grep -q "nix-daemon.sh" /etc/bash.bashrc.local; then
          echo "/etc/bash.bashrc.local still contains Nix snippet after uninstall"
          exit 1
        fi
        if grep -q "nix-daemon.sh" /etc/environment 2>/dev/null; then
          echo "/etc/environment still contains Nix BASH_ENV after uninstall"
          exit 1
        fi
      '';
    };
  };
  # For cure-self tests, we need to remove Nix from PATH before running the installer.
  # The initial install modifies shell profiles, so subsequent SSH commands have Nix in PATH.
  # This causes the installer's "nix already exists" check to fail.
  # We use env -i to run with a minimal environment, then restore essential variables.
  nix-installer-cure-install = ''
    # Run installer with PATH that excludes Nix directories
    PATH=$(echo "$PATH" | tr ':' '\n' | grep -v nix | tr '\n' ':' | sed 's/:$//') \
    RUST_BACKTRACE="full" ./nix-installer install --no-confirm --logger pretty --log-directive nix_installer=trace
  '';
  cureSelfCases = {
    cure-self-linux-working = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-no-nix-path = {
      preinstall = ''
        RUST_BACKTRACE="full" ./nix-installer install --no-confirm
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo rm -rf /nix/
      '';
      # This test removes /nix entirely, so nix-env won't be found anyway
      install = installCases.install-default.install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-missing-users = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo userdel nixbld1
        sudo userdel nixbld3
        sudo userdel nixbld16
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-missing-users-and-group = {
      preinstall = ''
        RUST_BACKTRACE="full" ./nix-installer install --no-confirm
        sudo mv /nix/receipt.json /nix/old-receipt.json
        for i in {1..32}; do
          sudo userdel "nixbld''${i}"
        done
        sudo groupdel nixbld
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-daemon-disabled = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo systemctl disable --now nix-daemon.socket
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-multi-broken-daemon-stopped = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo systemctl stop nix-daemon.socket
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-no-etc-nix = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo rm -rf /etc/nix
      '';
      install = nix-installer-cure-install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
    cure-self-linux-broken-unmodified-bashrc = {
      preinstall = ''
        ${nix-installer-install-quiet}
        sudo mv /nix/receipt.json /nix/old-receipt.json
        sudo sed -i '/# Nix/,/# End Nix/d' /etc/bash.bashrc
      '';
      # This test removes the Nix snippet from bash.bashrc, so Nix won't be in PATH
      install = installCases.install-default.install;
      check = installCases.install-default.check;
      uninstall = installCases.install-default.uninstall;
      uninstallCheck = installCases.install-default.uninstallCheck;
    };
  };
  # Cases to test uninstalling is complete even in the face of errors.
  uninstallCases =
    let
      uninstallFailExpected = ''
        if /nix/nix-installer uninstall --no-confirm; then
          echo "/nix/nix-installer uninstall exited with 0 during a uninstall failure test"
          exit 1
        else
          exit 0
        fi
      '';
    in
    {
      uninstall-users-and-groups-missing = {
        install = installCases.install-default.install;
        check = installCases.install-default.check;
        preuninstall = ''
          for i in $(seq 1 32); do
            sudo userdel nixbld$i
          done
          sudo groupdel nixbld
        '';
        uninstall = uninstallFailExpected;
        uninstallCheck = installCases.install-default.uninstallCheck;
      };
      uninstall-nix-conf-gone = {
        install = installCases.install-default.install;
        check = installCases.install-default.check;
        preuninstall = ''
          sudo rm -rf /etc/nix
        '';
        uninstall = uninstallFailExpected;
        uninstallCheck = installCases.install-default.uninstallCheck;
      };
    };

  images = {

    # End of standard support https://wiki.ubuntu.com/Releases
    "ubuntu-v22_04" = {
      image = import <nix/fetchurl.nix> {
        url = "https://cloud-images.ubuntu.com/releases/jammy/release-20260913/ubuntu-22.04-server-cloudimg-amd64-disk-kvm.img";
        hash = "sha256-vicNXW2BZzkUpj6DjdgPo1xXGpXEQBoOU43RWiBxVyE=";
      };
      system = "x86_64-linux";
    };

    "ubuntu-v24_04" = {
      image = import <nix/fetchurl.nix> {
        url = "https://cloud-images.ubuntu.com/releases/noble/release-20260911/ubuntu-24.04-server-cloudimg-amd64.img";
        hash = "sha256-YSssDMG8QTpsuMOP1hF5TK8PK0NsUAE9izeU2xKtc1Q=";
      };
      system = "x86_64-linux";
    };

    # End of life documentation https://docs.fedoraproject.org/en-US/releases/eol/
    "fedora-v43" = {
      image = import <nix/fetchurl.nix> {
        url = "https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2";
        hash = "sha256-hGV0yKl80tjcHyMQYtcxB8yFy7vaVjNeJkpG46bIqy8=";
      };
      system = "x86_64-linux";
    };

    # FIXME: Installs on Fedora 44 seem to be broken.

    "rocky-v8" = {
      image = import <nix/fetchurl.nix> {
        url = "https://dl.rockylinux.org/pub/rocky/8/images/x86_64/Rocky-8-GenericCloud-Base-8.10-20240528.0.x86_64.qcow2";
        hash = "sha256-5WBmxYYGGR6WGE3pqRg6OvM8Wby9h0DYsQygVKeonBQ=";
      };
      system = "x86_64-linux";
    };

    "rocky-v9" = {
      image = import <nix/fetchurl.nix> {
        url = "https://dl.rockylinux.org/pub/rocky/9/images/x86_64/Rocky-9-GenericCloud-Base-9.8-20260525.0.x86_64.qcow2";
        hash = "sha256-ksIGzG95DGFYMkfu/oeJD4goQgZiwXys8kfOx4q07sg=";
      };
      system = "x86_64-linux";
      extraQemuOpts = "-cpu Westmere-v2";
    };

    "opensuse-leap-v15_6" = {
      image = import <nix/fetchurl.nix> {
        url = "https://download.opensuse.org/distribution/leap/15.6/appliances/openSUSE-Leap-15.6-Minimal-VM.x86_64-15.6.0-Cloud-Build19.146.qcow2";
        hash = "sha256-ClcgQW1CP5iqyqeTpX1W7ARePdJc2IcTlSZgrQDaU70=";
      };
      system = "x86_64-linux";
    };

    # Official Arch Linux cloud image from https://geo.mirror.pkgbuild.com/images/
    # Built by https://gitlab.archlinux.org/archlinux/arch-boxes
    # FIXME: These links rot. Archive this somewhere.
    "archlinux-v20260915" = {
      image = import <nix/fetchurl.nix> {
        url = "https://geo.mirror.pkgbuild.com/images/v20260915.594445/Arch-Linux-x86_64-cloudimg.qcow2";
        hash = "sha256-18x8hqIbMtZnjAAUZHFPcfTvfg17u/ymXpkSOsWvwls=";
      };
      system = "x86_64-linux";
    };

  };

  makeTest =
    imageName: testName: test:
    let
      image = images.${imageName};
      pkgs = forSystem image.system ({ system, pkgs, ... }: pkgs);
    in
    with pkgs;
    runCommand "installer-test-${imageName}-${testName}"
      {
        buildInputs = [
          qemu_kvm
          openssh
          cdrkit
          colorized-logs
        ]
        ++ (if image ? extraBuildInputs then image.extraBuildInputs pkgs else [ ]);
        image = image.image;
        postBoot = image.postBoot or "";
        preinstallScript = test.preinstall or "echo \"Not Applicable\"";
        installScript = test.install;
        checkScript = test.check;
        uninstallScript = test.uninstall;
        preuninstallScript = test.preuninstall or "echo \"Not Applicable\"";
        uninstallCheckScript = test.uninstallCheck;
        installer = nix-installer-static;
      }
      ''
        shopt -s nullglob
        set -eu

        image_type=$(qemu-img info $image | sed 's/file format: \(.*\)/\1/; t; d')
        qemu-img create -b $image -F "$image_type" -f qcow2 ./disk.qcow2
        qemu-img resize ./disk.qcow2 +2G
        ssh-keygen -t ed25519 -f ./id_test

        # Configure our test user via cloud-init to get passwordless sudo and
        # the freshly generated ssh key.
        touch network-config
        touch meta-data
        cat << EOF > user-data
        #cloud-config
        users:
        - name: user
          shell: ${image.shell or "/bin/bash"}
          sudo: ALL=(ALL) NOPASSWD:ALL
          lock_passwd: true
          ssh_authorized_keys:
          - $(cat ./id_test.pub)
        bootcmd:
        # Workaround for Arch, which seems to block sshd on this?
        - systemctl stop systemd-time-wait-sync.service || true
        EOF

        genisoimage -output seed.img -volid cidata -rational-rock -joliet user-data meta-data network-config
        extra_qemu_opts="${image.extraQemuOpts or ""}"
        ssh_port=20022

        echo "Starting qemu..."
        qemu-kvm -m 4096 -nographic \
          -device virtio-rng-pci \
          -drive id=disk1,file=./disk.qcow2,if=virtio \
          -drive file=./seed.img,media=cdrom \
          -netdev user,id=net0,restrict=yes,hostfwd=tcp::$ssh_port-:22 -device virtio-net-pci,netdev=net0 \
          -run-with exit-with-parent=on \
          $extra_qemu_opts > >(ansi2txt) &

        qemu_pid=$!

        ssh_opts="-o StrictHostKeyChecking=no -i ./id_test"
        ssh="ssh -p $ssh_port -q $ssh_opts user@localhost"

        echo "Waiting for SSH..."
        for ((i = 0; i < 120; i++)); do
          echo "[ssh] Trying to connect..."
          if $ssh -- true; then
            echo "[ssh] Connected!"
            break
          fi
          if ! kill -0 $qemu_pid; then
            echo "qemu died unexpectedly"
            exit 1
          fi
          sleep 1
        done

        if [[ -n $postBoot ]]; then
          echo "Running post-boot commands..."
          $ssh "set -ex; $postBoot"
        fi

        echo "Copying installer..."
        scp -P $ssh_port $ssh_opts $installer/bin/nix-installer user@localhost:nix-installer

        echo "Running preinstall..."
        $ssh "set -eux; $preinstallScript"

        echo "Running installer..."
        $ssh "set -eux; $installScript"

        echo "Checking Nix installation..."
        $ssh "set -eux; $checkScript"

        echo "Running preuninstall..."
        $ssh "set -eux; $preuninstallScript"

        echo "Running Nix uninstallation..."
        $ssh "set -eux; $uninstallScript"

        echo "Checking Nix uninstallation..."
        $ssh "set -eux; $uninstallCheckScript"

        echo "Done!"
        touch $out
      '';

  makeTests =
    name: tests:
    builtins.mapAttrs (
      imageName: image:
      let
        doTests = builtins.removeAttrs tests (image.skip or [ ]);
      in
      rec {
        ${image.system} =
          (builtins.mapAttrs (testName: test: makeTest imageName testName test) doTests)
          // {
            "${name}" = (
              with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
              pkgs.releaseTools.aggregate {
                name = name;
                constituents = (pkgs.lib.mapAttrsToList (testName: test: makeTest imageName testName test) doTests);
              }
            );
          };
      }
    ) images;

  allCases = lib.recursiveUpdate installCases (lib.recursiveUpdate cureSelfCases uninstallCases);

  install-tests = makeTests "install" installCases;

  cure-self-tests = makeTests "cure-self" cureSelfCases;

  uninstall-tests = makeTests "uninstall" uninstallCases;

  all-tests = builtins.mapAttrs (imageName: image: {
    "x86_64-linux".all = (
      with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
      pkgs.releaseTools.aggregate {
        name = "all";
        constituents = [
          install-tests."${imageName}"."x86_64-linux".install
          cure-self-tests."${imageName}"."x86_64-linux".cure-self
          uninstall-tests."${imageName}"."x86_64-linux".uninstall
        ];
      }
    );
  }) images;

  joined-tests = lib.recursiveUpdate (lib.recursiveUpdate install-tests (lib.recursiveUpdate cure-self-tests uninstall-tests)) all-tests;

in
lib.recursiveUpdate joined-tests {
  all."x86_64-linux" =
    (
      with (forSystem "x86_64-linux" ({ system, pkgs, ... }: pkgs));
      pkgs.lib.mapAttrs (
        caseName: case:
        pkgs.releaseTools.aggregate {
          name = caseName;
          constituents = pkgs.lib.mapAttrsToList (
            name: value: value."x86_64-linux"."${caseName}" or ""
          ) joined-tests;
        }
      )
    )
      (
        allCases
        // {
          "cure-self" = { };
          "install" = { };
          "uninstall" = { };
          "all" = { };
        }
      );
}
