#!/usr/bin/env bash
set -euo pipefail

test "$(systemd-detect-virt --vm)" = kvm
podman load --input /var/tmp/image-update.tar
bootc switch --transport containers-storage localhost/nix-installer-bootc:updated
