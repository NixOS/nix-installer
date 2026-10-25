# Releasing nix-installer

No binary artifacts are released from this repo starting 2.36 (first stable release of the installer).
The only thing that needs to happen is producing a signed git tag.
The binary release in handled from the [NixOS/nix](https://github.com/NixOS/nix) repository, documented in
[release-process.md](https://github.com/NixOS/nix/blob/master/maintainers/release-process.md).

## Versioning Scheme

The installer is versioned separately from Nix, so regular patch releases of Nix usually don't need to apply any changes to nix-installer:

- **Major.Minor**: Matches the Nix version being installed (e.g., `2.36.x` installs Nix 2.36)
- **Patch**: Revision number for the installer itself

Examples:

- `2.36.0` - First release of the installer for Nix 2.36
- `2.36.1` - Bugfix release for Nix 2.36
- `2.37.0` - First release for Nix 2.37 (resets patch to 0)

## Creating a minor release for the corresponding Nix release.

<!-- Once the first release actually happens, write a script to do all this legwork. -->

* Create the Nix maintenance branch as described in [release-process.md](https://github.com/NixOS/nix/blob/master/maintainers/release-process.md).
  Come back when you've created the release branch with `officialRelease = true`.

* Make sure your `origin` remote points to the `NixOS/nix-installer` repository.

* Determine the intended release version (this should match the version of Nix you are releasing, e.g. 2.36.0):

  ```console
  $ export VERSION=$(grep -Po 'version = "\K.*?(?=-dev")' Cargo.toml)
  $ export MINOR_VERSION=$(echo "$VERSION" | cut -d. -f1,2)
  ```

* Create a minor release maintenance branch:

  ```console
  $ git checkout main
  $ git pull
  $ git checkout -b $MINOR_VERSION-maintenance
  ```

* Update the pinned nix release branch and drop the `-dev` suffix from the version in `Cargo.toml`:

  ```console
  $ sed -i "s:$VERSION-dev:$VERSION:g" ./Cargo.toml
  $ sed -i "s|github:NixOS/nix|github:NixOS/nix/$MINOR_VERSION-maintenance|g" ./flake.nix
  $ nix flake update nix
  $ nix run --inputs-from ./. -- nixpkgs#cargo update --workspace
  $ git diff
  $ git add Cargo.toml Cargo.lock flake.nix flake.lock
  $ git commit -m "Bump version to $VERSION"
  ```

* Push the release branch:

  ```console
  $ git push --set-upstream origin $MINOR_VERSION-maintenance
  ```

* Create a Hydra jobset by cloning the [`main` jobset](https://hydra.nixos.org/jobset/nix-installer/nix-installer).
  The jobset name is `nix-installer-$MINOR_VERSION`.
  This is only really needed for populating cache.nixos.org with the dependency closure and can be skipped.

* Make sure that the [CI](https://github.com/NixOS/nix-installer/actions/workflows/ci.yml) succeeds on the release branch
  after you've pushed it. The `main` branch should usually be sufficiently kept up to date with Nix `master` by dependabot,
  but if issues crop up -- fix them as needed.

* Tag the release:

  ```console
  $ git remote update origin
  $ git tag --force --sign $VERSION -m "Tagging release $VERSION"
  $ git push origin refs/tags/$VERSION
  ```

* Update the version to the next expected patch release:

  ```console
  $ export NEW_VERSION="$MINOR_VERSION.1"
  $ sed -i "s:$VERSION:$NEW_VERSION:g" ./Cargo.toml
  $ nix run --inputs-from ./. -- nixpkgs#cargo update --workspace
  $ git add Cargo.toml Cargo.lock
  $ git commit -m "Bump version to $NEW_VERSION"
  ```

* Create a draft [GitHub release](https://github.com/NixOS/nix-installer/releases/new) for the installer.

* You are done here, go back to the Nix release process. Come back when the binary release of Nix is uploaded.

* Publish the draft GitHub release you've created with relevant release notes for the installer itself.

<!-- TODO: gh cli for the draft release or github action. -->

* Update the version on `main` branch and open a PR with the bump.
  The CI checks will fail until you update Nix flake input to a Nix version from `master` with the corresponding version bump.

  ```console
  $ git checkout main
  $ git checkout -b bump-version
  $ export NEW_VERSION="next.nix.version" # e.g. "2.37.0"
  $ sed -i "s:$VERSION-dev:$NEW_VERSION-dev:g" ./Cargo.toml
  $ git add Cargo.toml Cargo.lock
  $ git commit -m "Bump version to $NEW_VERSION-dev"
  $ git push --set-upstream origin bump-version
  ```
