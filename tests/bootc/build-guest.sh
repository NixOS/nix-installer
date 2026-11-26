#!/usr/bin/env bash
set -euo pipefail

base_image=$1
persistence=$2

test "$(systemd-detect-virt --vm)" = kvm
cd ~/bootc-test

sudo podman build --format=oci \
  --tag localhost/nix-installer-bootc:installed \
  --build-arg "BOOTC_BASE_IMAGE=$base_image" \
  --build-arg "NIX_PERSISTENCE=$persistence" \
  --file Containerfile .

sudo podman build --format=oci \
  --tag localhost/nix-installer-bootc:test \
  --file VMContainerfile .

sudo podman build --format=oci \
  --tag localhost/nix-installer-bootc:updated \
  --file UpgradeContainerfile .

sudo podman save --format oci-archive \
  --output image-update.tar localhost/nix-installer-bootc:updated
sudo chown tester:tester image-update.tar
