#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 ]]; then
  echo "Usage: $0 /path/to/nix-installer" >&2
  exit 2
fi

test_dir=$(mktemp -d -t nix-installer-bootc.XXXXXX)
test_name=$(basename "$test_dir" | tr '[:upper:]' '[:lower:]')
test_image="$test_name:local"

cleanup() {
  status=$?
  docker image rm "$test_image" >/dev/null 2>&1 || true
  rm -rf "$test_dir"
  exit "$status"
}
trap cleanup EXIT

cp "$1" "$test_dir/nix-installer"
chmod +x "$test_dir/nix-installer"
cp "$(dirname "$0")/Containerfile" "$test_dir/Containerfile"
docker build --progress=plain -f "$test_dir/Containerfile" -t "$test_image" "$test_dir"

# Runtime/first-boot tests must run in a VM. Starting an entire bootc system
# in a privileged container can retrigger host devices and alter host sysctls.
echo "bootc image build and lint passed; first-boot testing requires a VM"
