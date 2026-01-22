# Releasing nix-installer

This document describes how to ship a new version of nix-installer, typically
when a new Nix version is released.

## Overview

The release process involves three main phases:

1. **Bump the Nix version** - Update flake.nix and Cargo.toml
2. **Wait for Hydra** - Let CI build binaries for all platforms
3. **Create the release** - Assemble binaries and publish a GitHub release

## Versioning Scheme

nix-installer follows the Nix project's versioning:

- **Major.Minor**: Matches the Nix version being installed (e.g., `2.33.x`
  installs Nix 2.33)
- **Patch**: Revision number for the installer itself

Examples:
- `2.33.0` - First release of the installer for Nix 2.33
- `2.33.1` - Bugfix release for Nix 2.33
- `2.34.0` - First release for Nix 2.34 (resets patch to 0)

## Step 1: Bump Nix Version

### Option A: GitHub Actions (Recommended)

1. Go to **Actions** → **Bump Nix Version**
2. Click **Run workflow** on the `main` branch
3. The workflow will:
   - Fetch the latest Nix version from GitHub
   - Update `flake.nix` to point to the new Nix version
   - Update `Cargo.toml` version (reset patch on major/minor bump, increment
     otherwise)
   - Run `nix flake update nix` and `cargo update`
   - Create a PR titled "Bump Nix version"

### Option B: Manual

```bash
# Run the bump script
nix run --inputs-from .# nixpkgs#python3 -- scripts/bump_nix_version.py

# Review the changes
git diff

# Commit and push
git checkout -b bump-nix-version
git add -A
git commit -m "Bump Nix version to X.Y.Z"
git push origin bump-nix-version
```

The script automatically:
- Fetches the latest stable Nix version from
  `https://api.github.com/repos/NixOS/nix/tags`
- Updates the Nix flake input in `flake.nix`
- Adjusts the crate version in `Cargo.toml`:
  - Major/minor bump → resets patch to 0
  - Same major/minor → increments patch

## Step 2: Wait for Hydra

After the PR is merged to `main`, Hydra will build the installer for all
supported platforms.

### Monitoring Builds

Check the build status at:
**https://hydra.nixos.org/jobset/nix-installer/nix-installer**

The jobset builds installers for:
- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin`
- `aarch64-darwin`

### Speeding Up Builds

To prioritize your builds over other queued jobs:

1. Go to your evaluation page:
   `https://hydra.nixos.org/eval/{EVAL_ID}`
2. Click the **Actions** dropdown (gear icon)
3. Select **"Bump builds to front of queue"**

This moves all builds from your evaluation to the front of the build queue,
which can significantly reduce wait times when Hydra is busy.

### What to Check

1. **All builds must be green** - The release script will fail if any build is
   incomplete
2. **Correct commit** - Verify the evaluation matches your merged commit (the
   release script validates this automatically)

You can view specific evaluations at:
`https://hydra.nixos.org/jobset/nix-installer/nix-installer/evals`

## Step 3: Create the Release

### Option A: GitHub Actions (Recommended)

1. Go to **Actions** → **Generate Installer Script**
2. Click **Run workflow**
3. (Optional) Enter a specific Hydra eval ID for testing, or leave blank to use
   the latest evaluation matching HEAD
4. The workflow will:
   - Verify Hydra builds are complete and match HEAD
   - Download all platform binaries
   - Substitute the version into `nix-installer.sh`
   - Create a **draft** GitHub release

### Option B: Manual

```bash
# Ensure you're on the commit that Hydra built
git checkout main
git pull

# Run the assembly script
nix run --inputs-from .# nixpkgs#python3 -- scripts/assemble_installer.py

# Or with a specific eval ID for testing
nix run --inputs-from .# nixpkgs#python3 -- scripts/assemble_installer.py 12345
```

### Publish the Release

1. Go to the **Releases** page on GitHub
2. Find the draft release created by the script
3. Review the release notes (auto-generated from changelog)
4. Edit the title/notes if needed
5. Click **Publish release**

## Post-Release

### Verify the Release

Test the installer script works:

```bash
curl -L https://github.com/NixOS/nix-installer/releases/download/X.Y.Z/nix-installer.sh | sh -s -- --help
```

## Reference

- Hydra jobset: https://hydra.nixos.org/jobset/nix-installer/nix-installer
- Bump script: `scripts/bump_nix_version.py`
- Release script: `scripts/assemble_installer.py`
- CI workflows: `.github/workflows/bump-nix-version.yml`,
  `.github/workflows/release-script.yml`
