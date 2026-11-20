#!/bin/sh
# Seed the persistent Nix store from this image's /nix.
#
# The copy is staged next to the persistence directory and renamed into
# place, so an interrupted first boot cannot leave a partial store behind.
set -eu
persistence=$1
staging=$2

if [ -e "$persistence" ]; then
  exit 0
fi
rm -rf -- "$staging"
/usr/bin/systemd-tmpfiles --create /usr/lib/nix-installer/nix-store-copy.conf
mv -T -- "$staging" "$persistence"
