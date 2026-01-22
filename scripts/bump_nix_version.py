#!/usr/bin/env python3
"""Bump nix-installer to track the latest Nix version."""

import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request


def require(condition: bool, message: str) -> None:
    """Assert that condition is true, raising RuntimeError if not."""
    if not condition:
        raise RuntimeError(message)


def get_latest_nix_version() -> str:
    """Fetch the latest Nix version tag from GitHub."""
    request = urllib.request.Request(
        "https://api.github.com/repos/NixOS/nix/tags",
        headers={"Accept": "application/vnd.github+json"},
    )
    with urllib.request.urlopen(request) as response:
        tags = json.loads(response.read())
    return next(
        tag["name"] for tag in tags if re.match(r"^\d+\.\d+\.\d+$", tag["name"])
    )


def read_file(path: str) -> str:
    with open(path) as file:
        return file.read()


def write_file(path: str, content: str) -> None:
    dir_name = os.path.dirname(path) or "."
    with tempfile.NamedTemporaryFile(mode="w", dir=dir_name, delete=False) as file:
        file.write(content)
        temp_path = file.name
    os.rename(temp_path, path)


def parse_version(version_str: str) -> tuple[int, int, int]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", version_str)
    require(match is not None, f"Invalid version: {version_str}")
    assert match  # for type narrowing
    return int(match[1]), int(match[2]), int(match[3])


def main() -> None:
    flake_content = read_file("flake.nix")
    cargo_content = read_file("Cargo.toml")

    match = re.search(r"github:NixOS/nix/(\d+\.\d+\.\d+)", flake_content)
    require(match is not None, "Could not find nix version in flake.nix")
    assert match  # for type narrowing
    current_nix = match[1]
    match = re.search(r'^version\s*=\s*"(\d+\.\d+\.\d+)"', cargo_content, re.M)
    require(match is not None, "Could not find version in Cargo.toml")
    assert match  # for type narrowing
    current_crate = match[1]
    latest_nix = get_latest_nix_version()

    if current_nix == latest_nix:
        print(f"Already at latest Nix version {current_nix}")
        sys.exit(0)

    current_nix_version = parse_version(current_nix)
    current_crate_version = parse_version(current_crate)
    latest_nix_version = parse_version(latest_nix)

    # Reset patch on major/minor bump, else increment
    if current_nix_version[:2] != latest_nix_version[:2]:
        new_crate = f"{latest_nix_version[0]}.{latest_nix_version[1]}.0"
    else:
        new_crate = f"{current_crate_version[0]}.{current_crate_version[1]}.{current_crate_version[2] + 1}"

    print(f"Nix: {current_nix} -> {latest_nix}")
    print(f"Crate: {current_crate} -> {new_crate}")

    write_file(
        "flake.nix",
        flake_content.replace(
            f"github:NixOS/nix/{current_nix}", f"github:NixOS/nix/{latest_nix}"
        ),
    )
    write_file(
        "Cargo.toml",
        re.sub(
            rf'^(version\s*=\s*"){current_crate}"',
            rf'\g<1>{new_crate}"',
            cargo_content,
            count=1,
            flags=re.M,
        ),
    )
    subprocess.run(["nix", "flake", "update", "nix"], check=True)
    subprocess.run(["cargo", "update", "--workspace"], check=True)


if __name__ == "__main__":
    main()
