#!/usr/bin/env bash
set -euo pipefail

test "$(systemd-detect-virt --vm)" = kvm
test "$(hostname)" = bootc-builder
test "$(lsblk -bdno SIZE /dev/vdb)" = 32212254720
test -z "$(lsblk -no MOUNTPOINTS /dev/vdb | tr -d '[:space:]')"

sudo podman run --rm --privileged --pid=host --security-opt label=disable \
  --volume /var/lib/containers:/var/lib/containers \
  --volume /home/tester/bootc-test/id_ed25519.pub:/ssh.pub:ro \
  localhost/nix-installer-bootc:test bootc install to-disk \
  --generic-image --filesystem ext4 --skip-fetch-check \
  --target-transport containers-storage \
  --target-imgref localhost/nix-installer-bootc:test \
  --root-ssh-authorized-keys /ssh.pub --karg console=ttyS0 /dev/vdb
