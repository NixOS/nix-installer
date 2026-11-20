# Testing the bootc planner

Build the static installer with `nix build .#nix-installer-static`.

## Image builds

```sh
tests/bootc/test.sh result/bin/nix-installer

BOOTC_BASE_IMAGE=quay.io/fedora-ostree-desktops/silverblue:43 \
  NIX_PERSISTENCE=/var/lib/nix-installer-test \
  tests/bootc/test.sh result/bin/nix-installer
```

This checks installation without `/root`, the receipt, offline systemd
configuration and `bootc container lint`. The temporary image and build directory
are removed afterward. CI runs Fedora 43 and Silverblue 43 with default and
custom persistence paths on x86_64 and ARM64. Use an image digest in
`BOOTC_BASE_IMAGE` to reproduce a particular build.

## VM tests

On x86_64 Linux with access to `/dev/kvm`:

```sh
nix shell --inputs-from . nixpkgs#python3 nixpkgs#qemu nixpkgs#cdrkit \
  --command python3 tests/bootc/vm-test.py result/bin/nix-installer \
  --base-image quay.io/fedora-ostree-desktops/silverblue:43 \
  --persistence /var/lib/nix-installer-test \
  --work-dir /tmp/nix-installer-bootc-silverblue
```

Use a new work directory for each run. The script downloads a checksum-verified
Fedora Cloud image and creates a builder VM and a separate bootc disk. To reuse
the cloud image, pass `--cloud-image /path/to/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2`.
The VMs run sequentially with four CPUs each: 6 GiB RAM for the builder and 4 GiB
for bootc. They use virtual disks and localhost SSH; privileged container and
deployment commands run inside the builder VM.

The tests cover first boot, reboot, image replacement and rollback. Each phase
checks the mount, build-user IDs and SELinux, then builds through the daemon as
an unprivileged user. Store contents, profile links, ownership and previous
build outputs must survive. The update must boot a different image, and rollback
must restore the original image digest.

`VMContainerfile` enables SSH and selects `multi-user.target` for testing.
VMs shut down when the script exits. Logs, SSH keys and disks remain in the work
directory; `result.json` records successful runs and their inputs and image digests.

VM tests run locally; CI runs image builds and lint. Image replacement keeps the
same Fedora release and Nix version, with `sandbox = false`. Release upgrades,
Nix version changes and sandboxed builds still need testing.
