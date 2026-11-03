#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 ]]; then
  printf 'Usage: %s INSTALLER\n' "$0" >&2
  exit 2
fi

test_dir=$(mktemp -d -t nix-installer-bootc.XXXXXX)
test_image="nix-installer-bootc:${test_dir##*.}"

cleanup() {
  local exit_code=$?
  docker image rm "$test_image" >/dev/null 2>&1 || true
  rm -rf -- "$test_dir"
  exit "$exit_code"
}
trap cleanup EXIT

cp "$1" "$test_dir/nix-installer"
chmod +x "$test_dir/nix-installer"
cp "$(dirname "$0")/Containerfile" "$test_dir/Containerfile"
build_args=()
if [[ -n ${BOOTC_BASE_IMAGE:-} ]]; then
  build_args+=(--build-arg "BOOTC_BASE_IMAGE=$BOOTC_BASE_IMAGE")
fi
if [[ -n ${NIX_PERSISTENCE:-} ]]; then
  build_args+=(--build-arg "NIX_PERSISTENCE=$NIX_PERSISTENCE")
fi
docker build --pull --progress=plain \
  --file "$test_dir/Containerfile" \
  --tag "$test_image" \
  "${build_args[@]}" "$test_dir"

printf '%s\n' 'bootc image build and lint passed'
