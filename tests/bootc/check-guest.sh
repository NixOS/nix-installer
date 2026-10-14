#!/usr/bin/env bash
set -Eeuo pipefail
trap 'printf "Check failed at line %s: %s\n" "$LINENO" "$BASH_COMMAND" >&2' ERR

readonly state_dir=/var/lib/nix-installer-vm-test
readonly nix_profile=/nix/var/nix/profiles/default

check_system() {
  local persistence=$1

  test "$(systemd-detect-virt --vm)" = kvm
  test -e /run/ostree-booted
  test "$(findmnt -n -o TARGET --target /nix)" = /nix
  test "$(stat -c '%d:%i' /nix)" = "$(stat -c '%d:%i' "$persistence")"
  systemctl is-active nix-store-setup.service nix.mount nix-store-label.service nix-daemon.socket
  test "$(getenforce)" = Enforcing
  test "$(id -u nixbld1)" = 30001
  test "$(id -u nixbld32)" = 30032
  test "$(id -g nixbld1)" = 30000
}

record_initial_state() {
  mkdir -p "$state_dir"
  readlink -f "$nix_profile" >"$state_dir/profile"
  readlink "$nix_profile" >"$state_dir/profile-link"
  printf '%s\n' persistent >/nix/bootc-vm-test-marker
  install -o nixbld1 -g nixbld -m 0600 /dev/null /nix/bootc-vm-owned-marker
  printf '%s\n' owned >/nix/bootc-vm-owned-marker
}

check_build_output() {
  local phase=$1 output
  output=$(cat "$state_dir/$phase-output")
  test "$(cat "$output")" = "$phase-ok"
}

check_persistent_state() {
  test "$(cat /proc/sys/kernel/random/boot_id)" != "$(cat "$state_dir/boot-id")"
  test "$(readlink -f "$nix_profile")" = "$(cat "$state_dir/profile")"
  test "$(readlink "$nix_profile")" = "$(cat "$state_dir/profile-link")"
  test "$(cat /nix/bootc-vm-test-marker)" = persistent
  test "$(cat /nix/bootc-vm-owned-marker)" = owned
  test "$(stat -c '%u:%g' /nix/bootc-vm-owned-marker)" = 30001:30000
  test ! -e /nix/bootc-vm-image-only-marker
  check_build_output first-boot
}

build_as_user() {
  local phase=$1 output

  install -d -m 0700 -o nobody /tmp/nix-installer-vm-test-user
  output=$(runuser -u nobody -- env HOME=/tmp/nix-installer-vm-test-user NIX_REMOTE=daemon \
    "$nix_profile/bin/nix-build" --no-out-link --no-substitute \
    --argstr phase "$phase" /var/tmp/nix-installer-vm-build.nix)
  printf '%s\n' "$output" >"$state_dir/$phase-output"
  check_build_output "$phase"
  systemctl is-active nix-daemon.service
  printf '%s passed: %s\n' "$phase" "$output"
}

main() {
  if [[ $# != 2 ]]; then
    printf 'Usage: %s PHASE PERSISTENCE\n' "$0" >&2
    return 2
  fi

  local phase=$1 persistence=$2
  case "$phase" in
  first-boot | reboot | updated | rollback) ;;
  *)
    printf 'Unknown phase: %s\n' "$phase" >&2
    return 2
    ;;
  esac

  check_system "$persistence"
  if [[ $phase == first-boot ]]; then
    record_initial_state
  else
    check_persistent_state
  fi

  case "$phase" in
  updated)
    test "$(cat /usr/lib/nix-installer-vm-image)" = updated
    ;;
  rollback)
    test ! -e /usr/lib/nix-installer-vm-image
    check_build_output updated
    ;;
  esac

  build_as_user "$phase"
  cat /proc/sys/kernel/random/boot_id >"$state_dir/boot-id"
}

main "$@"
