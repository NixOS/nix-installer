#!/usr/bin/env python3
"""Test bootc installation, reboot, image replacement and rollback in QEMU."""

import argparse
import hashlib
import json
import os
import platform
import shlex
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from collections.abc import Iterator, Sequence
from contextlib import contextmanager, nullcontext
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

TEST_DIR = Path(__file__).resolve().parent
CLOUD_URL = "https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2"
CLOUD_SHA256 = "846574c8a97cd2d8dc1f231062d73107cc85cbbbda56335e264a46e3a6c8ab2f"


def run_command(
    command: Sequence[str | Path],
    *,
    log: Path | None = None,
    input_text: str | None = None,
    accepted: tuple[int, ...] = (0,),
    timeout: int = 1800,
) -> None:
    output_context = log.open("w") if log is not None else nullcontext()
    with output_context as output:
        result = subprocess.run(
            command,
            input=input_text,
            text=True,
            stdout=output,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
    if result.returncode in accepted:
        return
    if log:
        raise RuntimeError(
            f"Command failed with exit code {result.returncode}; see {log}"
        )
    raise subprocess.CalledProcessError(result.returncode, command)


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def wait_for_exit(process: subprocess.Popen, timeout: int) -> bool:
    try:
        process.wait(timeout=timeout)
        return True
    except subprocess.TimeoutExpired:
        return False


@dataclass
class VirtualMachine:
    directory: Path
    name: str
    user: str
    disk: Path
    extra_drives: list[str] = field(default_factory=list)
    memory: int = 4096
    port: int = field(default_factory=free_port)
    process: subprocess.Popen | None = field(default=None, init=False)

    def ssh_options(self) -> list[str]:
        options = ["-F", "/dev/null", "-i", str(self.directory / "id_ed25519")]
        for option in (
            "BatchMode=yes",
            "IdentitiesOnly=yes",
            "StrictHostKeyChecking=accept-new",
            "ConnectTimeout=5",
            f"UserKnownHostsFile={self.directory / 'known_hosts'}",
        ):
            options.extend(["-o", option])
        return options

    def ssh_command(self, command: Sequence[str]) -> list[str]:
        return [
            "ssh",
            *self.ssh_options(),
            "-p",
            str(self.port),
            f"{self.user}@127.0.0.1",
            shlex.join(command),
        ]

    def run(
        self, command: Sequence[str], *, log: str, accepted: tuple[int, ...] = (0,)
    ) -> None:
        run_command(
            self.ssh_command(command), log=self.directory / log, accepted=accepted
        )

    def run_script(self, script: Path, *args: str, log: str) -> None:
        run_command(
            self.ssh_command(["bash", "-s", "--", *args]),
            input_text=script.read_text(),
            log=self.directory / log,
        )

    def upload(self, source: Path, destination: str) -> None:
        self._copy(str(source), f"{self.user}@127.0.0.1:{destination}")

    def download(self, source: str, destination: Path) -> None:
        self._copy(f"{self.user}@127.0.0.1:{source}", str(destination))

    def _copy(self, source: str, destination: str) -> None:
        run_command(
            ["scp", *self.ssh_options(), "-P", str(self.port), source, destination]
        )

    def start(self) -> None:
        command = ["qemu-system-x86_64"]
        options = (
            ("-name", f"nix-installer-{self.name}"),
            ("-machine", "q35,accel=kvm"),
            ("-cpu", "host"),
            ("-smp", "4"),
            ("-m", str(self.memory)),
            ("-display", "none"),
            ("-vga", "none"),
            ("-monitor", "none"),
            ("-serial", f"file:{self.directory / (self.name + '-console.log')}"),
            ("-device", "virtio-rng-pci"),
            ("-drive", f"file={self.disk},if=virtio,format=qcow2"),
            ("-netdev", f"user,id=net0,hostfwd=tcp:127.0.0.1:{self.port}-:22"),
            ("-device", "virtio-net-pci,netdev=net0"),
        )
        for flag, value in options:
            command.extend([flag, value])
        for drive in self.extra_drives:
            command.extend(["-drive", drive])
        write_json(self.directory / f"{self.name}-command.json", command)
        with (self.directory / f"{self.name}-qemu.log").open("w") as output:
            self.process = subprocess.Popen(
                command, stdout=output, stderr=subprocess.STDOUT
            )

    def wait(self, previous_boot_id: str | None = None) -> None:
        if self.process is None:
            raise RuntimeError(f"{self.name} has not been started")
        deadline = time.monotonic() + 300
        with (self.directory / f"{self.name}-ssh.log").open("a") as output:
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise RuntimeError(
                        f"{self.name} exited; see its console and QEMU logs"
                    )
                try:
                    result = subprocess.run(
                        self.ssh_command(["cat", "/proc/sys/kernel/random/boot_id"]),
                        stdout=subprocess.PIPE,
                        stderr=output,
                        text=True,
                        timeout=15,
                    )
                except subprocess.TimeoutExpired:
                    time.sleep(2)
                    continue
                if result.returncode == 0 and result.stdout.strip() != previous_boot_id:
                    return
                time.sleep(2)
        raise RuntimeError(
            f"Timed out waiting for {self.name}; see its console and SSH logs"
        )

    def reboot(self) -> None:
        self.run(["cat", "/proc/sys/kernel/random/boot_id"], log="reboot-boot-id.log")
        previous_boot_id = (self.directory / "reboot-boot-id.log").read_text().strip()
        self.run(["reboot"], log="reboot-command.log", accepted=(0, 255))
        self.wait(previous_boot_id)

    def stop(self) -> None:
        process = self.process
        if process is None:
            return
        if process.poll() is not None:
            self.process = None
            return
        command = ["systemctl", "poweroff", "--no-block"]
        if self.user != "root":
            command = ["sudo", "-n", *command]
        with (self.directory / f"{self.name}-shutdown.log").open("w") as output:
            try:
                subprocess.run(
                    self.ssh_command(command),
                    stdout=output,
                    stderr=subprocess.STDOUT,
                    timeout=15,
                )
            except subprocess.TimeoutExpired:
                pass
        if not wait_for_exit(process, 60):
            process.terminate()
            if not wait_for_exit(process, 10):
                process.kill()
                process.wait()
        self.process = None

    @contextmanager
    def running(self) -> Iterator[None]:
        try:
            self.start()
            self.wait()
            yield
        finally:
            self.stop()


def prepare_disks(directory: Path, cloud_image: Path | None) -> None:
    if cloud_image is None:
        cloud_image = directory / "fedora-cloud.qcow2"
        with (
            urllib.request.urlopen(CLOUD_URL, timeout=60) as source,
            cloud_image.open("wb") as target,
        ):
            shutil.copyfileobj(source, target)
    cloud_image = cloud_image.resolve()
    if sha256(cloud_image) != CLOUD_SHA256:
        raise RuntimeError(
            "Fedora Cloud image checksum does not match the pinned image"
        )
    run_command(
        [
            "qemu-img",
            "create",
            "-f",
            "qcow2",
            "-F",
            "qcow2",
            "-b",
            cloud_image,
            directory / "builder.qcow2",
            "60G",
        ]
    )
    run_command(["qemu-img", "create", "-f", "qcow2", directory / "bootc.qcow2", "30G"])


def prepare_cloud_init(directory: Path) -> None:
    run_command(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", directory / "id_ed25519"]
    )
    public_key = (directory / "id_ed25519.pub").read_text().strip()
    (directory / "user-data").write_text(f"""#cloud-config
hostname: bootc-builder
users:
  - name: tester
    groups: wheel
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - {public_key}
ssh_pwauth: false
packages:
  - podman
""")
    (directory / "meta-data").write_text(
        "instance-id: nix-installer-bootc-test\nlocal-hostname: bootc-builder\n"
    )
    run_command(
        [
            "genisoimage",
            "-output",
            directory / "seed.iso",
            "-volid",
            "cidata",
            "-joliet",
            "-rock",
            directory / "user-data",
            directory / "meta-data",
        ],
        log=directory / "cloud-init-iso.log",
    )


def build_images(
    builder: VirtualMachine, installer: Path, base_image: str, persistence: str
) -> None:
    builder.run(["mkdir", "-p", "bootc-test"], log="builder-setup.log")
    shutil.copy2(installer, builder.directory / "nix-installer")
    for source in (
        builder.directory / "nix-installer",
        builder.directory / "id_ed25519.pub",
        TEST_DIR / "Containerfile",
        TEST_DIR / "VMContainerfile",
        TEST_DIR / "UpgradeContainerfile",
    ):
        builder.upload(source, "bootc-test/")
    builder.run_script(
        TEST_DIR / "build-guest.sh", base_image, persistence, log="image-build.log"
    )
    builder.download(
        "bootc-test/image-update.tar", builder.directory / "image-update.tar"
    )


def check_guest(guest: VirtualMachine, phase: str, persistence: str) -> dict[str, Any]:
    print(f"Checking {phase}...", flush=True)
    guest.run_script(
        TEST_DIR / "check-guest.sh", phase, persistence, log=f"{phase}-check.log"
    )
    status_file = guest.directory / f"{phase}-status.json"
    guest.run(["bootc", "status", "--json"], log=status_file.name)
    guest.run(
        [
            "journalctl",
            "-b",
            "--no-pager",
            "-u",
            "nix-store-setup.service",
            "-u",
            "nix.mount",
            "-u",
            "nix-store-label.service",
            "-u",
            "nix-daemon.service",
        ],
        log=f"{phase}-services.log",
    )
    shutil.copy2(
        guest.directory / "bootc-console.log", guest.directory / f"{phase}-console.log"
    )
    return json.loads(status_file.read_text())


def image_digest(status: dict[str, Any], deployment: str = "booted") -> str:
    return status["status"][deployment]["image"]["imageDigest"]


def exercise_guest(guest: VirtualMachine, persistence: str) -> dict[str, str]:
    guest.upload(TEST_DIR / "build-test.nix", "/var/tmp/nix-installer-vm-build.nix")
    initial_digest = image_digest(check_guest(guest, "first-boot", persistence))
    guest.reboot()
    check_guest(guest, "reboot", persistence)

    print("Staging image update...", flush=True)
    guest.upload(guest.directory / "image-update.tar", "/var/tmp/image-update.tar")
    guest.run_script(TEST_DIR / "update-guest.sh", log="update-stage.log")
    guest.reboot()
    updated = check_guest(guest, "updated", persistence)
    updated_digest = image_digest(updated)
    if updated_digest == initial_digest:
        raise RuntimeError("Image update did not change the booted image digest")
    if image_digest(updated, "rollback") != initial_digest:
        raise RuntimeError(
            "Image update did not retain the original image for rollback"
        )

    print("Staging rollback...", flush=True)
    guest.run(["bootc", "rollback"], log="rollback-stage.log")
    guest.reboot()
    rolled_back_digest = image_digest(check_guest(guest, "rollback", persistence))
    if rolled_back_digest != initial_digest:
        raise RuntimeError("Rollback did not restore the original image digest")
    return {
        "initial": initial_digest,
        "updated": updated_digest,
        "rollback": rolled_back_digest,
    }


def existing_file(value: str) -> Path:
    path = Path(value).resolve()
    if not path.is_file():
        raise argparse.ArgumentTypeError(f"File does not exist: {value}")
    return path


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("installer", type=existing_file)
    parser.add_argument("--base-image", required=True)
    parser.add_argument("--persistence", default="/var/lib/nix")
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--cloud-image", type=existing_file)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("The VM tests require x86_64 Linux")
    if not os.access("/dev/kvm", os.R_OK | os.W_OK):
        parser.error("The current user needs read/write access to /dev/kvm")
    for tool in (
        "qemu-system-x86_64",
        "qemu-img",
        "genisoimage",
        "ssh",
        "scp",
        "ssh-keygen",
    ):
        if shutil.which(tool) is None:
            parser.error(f"Missing required tool: {tool}")
    args.work_dir = args.work_dir.resolve()
    if "," in str(args.work_dir):
        parser.error("QEMU drive paths cannot contain commas")
    if args.work_dir.exists():
        parser.error("Use a new --work-dir to preserve earlier logs and virtual disks")
    return args


def run_test(args: argparse.Namespace) -> None:
    directory = args.work_dir
    directory.mkdir(mode=0o700, parents=True)
    metadata: dict[str, Any] = {
        "installer_sha256": sha256(args.installer),
        "base_image": args.base_image,
        "persistence": args.persistence,
    }
    write_json(directory / "inputs.json", metadata)
    print("Preparing builder VM...", flush=True)
    prepare_disks(directory, args.cloud_image)
    prepare_cloud_init(directory)
    builder = VirtualMachine(
        directory,
        "builder",
        "tester",
        directory / "builder.qcow2",
        memory=6144,
        extra_drives=[
            f"file={directory / 'bootc.qcow2'},if=virtio,format=qcow2",
            f"file={directory / 'seed.iso'},media=cdrom,readonly=on",
        ],
    )
    with builder.running():
        builder.run(
            ["sudo", "cloud-init", "status", "--wait"],
            log="cloud-init.log",
            accepted=(0, 2),
        )
        print("Building bootc images inside the VM...", flush=True)
        build_images(builder, args.installer, args.base_image, args.persistence)
        print("Deploying to the virtual disk...", flush=True)
        builder.run_script(TEST_DIR / "deploy-guest.sh", log="deployment.log")

    guest = VirtualMachine(directory, "bootc", "root", directory / "bootc.qcow2")
    with guest.running():
        metadata["digests"] = exercise_guest(guest, args.persistence)
    metadata.update(
        phases=["first-boot", "reboot", "updated", "rollback"], result="passed"
    )
    write_json(directory / "result.json", metadata)
    print(f"All VM checks passed. Logs: {directory}", flush=True)


def main() -> int:
    args = parse_arguments()
    try:
        run_test(args)
        return 0
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"{error}\nLogs and virtual disks: {args.work_dir}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
