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
        url = "https://app.vagrantup.com/generic/boxes/ubuntu2204/versions/4.1.12/providers/libvirt.box";
        hash = "sha256-HNll0Qikw/xGIcogni5lz01vUv+R3o8xowP2EtqjuUQ=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
    };

    "ubuntu-v24_04" = {
      image = import <nix/fetchurl.nix> {
        url = "https://vagrantcloud.com/bento/boxes/ubuntu-24.04/versions/202502.21.0/providers/libvirt/amd64/vagrant.box";
        hash = "sha256-nXerG+g7DG2EszsczaOeVMkbpPOTXGKa+KdYHvF9jq8=";
      };
      rootDisk = "box_0.img";
      system = "x86_64-linux";
    };

    "fedora-v36" = {
      image = import <nix/fetchurl.nix> {
        url = "https://app.vagrantup.com/generic/boxes/fedora36/versions/4.1.12/providers/libvirt.box";
        hash = "sha256-rxPgnDnFkTDwvdqn2CV3ZUo3re9AdPtSZ9SvOHNvaks=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
    };

    "fedora-v37" = {
      image = import <nix/fetchurl.nix> {
        url = "https://app.vagrantup.com/generic/boxes/fedora37/versions/4.2.14/providers/libvirt.box";
        hash = "sha256-rxPgnDnFkTDwvdqn2CV3ZUo3re9AdPtSZ9SvOHNvaks=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
    };

    "rocky-v8" = {
      image = import <nix/fetchurl.nix> {
        url = "https://app.vagrantup.com/generic/boxes/rocky8/versions/4.1.12/providers/libvirt.box";
        hash = "sha256-IAjRT9h1T3Fc/1+aIbKlPLn3uP29cqM+JRVoFztHWV4=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
    };

    "rocky-v9" = {
      image = import <nix/fetchurl.nix> {
        url = "https://app.vagrantup.com/generic/boxes/rocky9/versions/4.1.12/providers/libvirt.box";
        hash = "sha256-1M7JDMYYYwAwIBvDOsixH/umefPvZ0bCaWzSG1DwX5Y=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
      extraQemuOpts = "-cpu Westmere-v2";
    };

    "opensuse-leap-v15_6" = {
      image = import <nix/fetchurl.nix> {
        url = "https://download.opensuse.org/distribution/leap/15.6/appliances/Leap-15.6.x86_64-15.6-libvirt-Build19.53.vagrant.libvirt.box";
        hash = "sha256-dBem1TOURmFkX80y+aKlJ3OW7hblA5tNYdrTWheuZa4=";
      };
      rootDisk = "box.img";
      system = "x86_64-linux";
    };

    # Official Arch Linux cloud image from https://geo.mirror.pkgbuild.com/images/
    # Built by https://gitlab.archlinux.org/archlinux/arch-boxes
    # Uses virt-customize to inject vagrant user + SSH key since the cloud
    # image relies on cloud-init (which we disable) and OpenSSH 10.x
    # disables password auth by default.
    "archlinux-v20260115" = {
      image = import <nix/fetchurl.nix> {
        url = "https://geo.mirror.pkgbuild.com/images/v20260115.482142/Arch-Linux-x86_64-cloudimg-20260115.482142.qcow2";
        hash = "sha256-kYz1wyQZmkNgmPJv+BnMqCDLrejCChJ29BE0yzM77uM=";
      };
      extraBuildInputs = pkgs: [ pkgs.guestfs-tools ];
      setupScript = ''
        echo "Preparing Arch Linux cloud image..."
        cp "$image" ./disk.qcow2
        chmod 644 ./disk.qcow2
        qemu-img resize ./disk.qcow2 20G

        vagrant_pubkey="$(ssh-keygen -y -f ./vagrant_insecure_key)"
        virt-customize -a ./disk.qcow2 --no-network \
          --run-command 'useradd -m -G wheel -s /bin/bash vagrant' \
          --run-command 'mkdir -p /home/vagrant/.ssh' \
          --run-command "echo '$vagrant_pubkey' > /home/vagrant/.ssh/authorized_keys" \
          --run-command 'chmod 700 /home/vagrant/.ssh' \
          --run-command 'chmod 600 /home/vagrant/.ssh/authorized_keys' \
          --run-command 'chown -R vagrant:vagrant /home/vagrant/.ssh' \
          --run-command 'echo "vagrant ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/vagrant' \
          --run-command 'systemctl disable cloud-init-main.service cloud-init-local.service cloud-init-network.service cloud-config.service cloud-final.service' \
          --run-command 'systemctl disable pacman-init.service systemd-time-wait-sync.service || true' \
          --run-command 'systemctl enable sshd.service || true' \
          --run-command 'systemctl enable systemd-networkd.service || true' \
          --run-command 'systemctl enable systemd-resolved.service || true' \
          --run-command 'touch /etc/machine-id' \
          --run-command 'ssh-keygen -A' \
          --run-command 'sed -i "s/^GRUB_TIMEOUT=.*/GRUB_TIMEOUT=0/" /etc/default/grub && grub-mkconfig -o /boot/grub/grub.cfg' \
          --write '/etc/systemd/network/20-ethernet.network:[Match]
        Name=eth0

        [Network]
        DHCP=yes
        '
      '';
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

        if ! [ -e ./vagrant_insecure_key ]; then
          cp ${./vagrant_insecure_key} vagrant_insecure_key
        fi
        chmod 0400 ./vagrant_insecure_key

        ${
          if (image.setupScript or "") != "" then
            image.setupScript
          else
            ''
              echo "Unpacking Vagrant box $image..."
              tar xvf $image

              image_type=$(qemu-img info ${image.rootDisk or "box.img"} | sed 's/file format: \(.*\)/\1/; t; d')

              qemu-img create -b ./${image.rootDisk or "box.img"} -F "$image_type" -f qcow2 ./disk.qcow2
            ''
        }

        extra_qemu_opts="${image.extraQemuOpts or ""}"

        # Add the config disk, required by the Ubuntu images.
        config_drive=$(echo *configdrive.vmdk || true)
        if [[ -n $config_drive ]]; then
          extra_qemu_opts+=" -drive id=disk2,file=$config_drive,if=virtio"
        fi

        echo "Starting qemu..."
        qemu-kvm -m 4096 -nographic \
          -device virtio-rng-pci \
          -drive id=disk1,file=./disk.qcow2,if=virtio \
          -netdev user,id=net0,restrict=yes,hostfwd=tcp::20022-:22 -device virtio-net-pci,netdev=net0 \
          $extra_qemu_opts &
        qemu_pid=$!
        trap "kill $qemu_pid" EXIT

        ssh_opts="-o StrictHostKeyChecking=no -o HostKeyAlgorithms=+ssh-rsa -o PubkeyAcceptedKeyTypes=+ssh-rsa -i ./vagrant_insecure_key"
        ssh="ssh -p 20022 -q $ssh_opts vagrant@localhost"

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
        scp -P 20022 $ssh_opts $installer/bin/nix-installer vagrant@localhost:nix-installer

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
