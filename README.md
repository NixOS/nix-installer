# Experimental Nix Installer

Note, this is different from the Determinate Nix Installer, available at https://github.com/DeterminateSystems/nix-installer.

## If you're having a problem with installing Nix, this repository is almost certainly the wrong place to record issues.

If you used the **official Nix install scripts**, report issues at https://github.com/NixOS/nix/issues.

If you used the **Determinate Nix Installer**, report issues at https://github.com/DeterminateSystems/nix-installer.

---

[![Crates.io](https://img.shields.io/crates/v/nix-installer)](https://crates.io/crates/nix-installer)
[![Docs.rs](https://img.shields.io/docsrs/nix-installer)](https://docs.rs/nix-installer/latest/nix_installer)


This one-liner is the quickest way to get started on any supported system:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install
```



| Platform                                                             |    Multi user?    | `root` only |     Maturity      |
| -------------------------------------------------------------------- | :---------------: | :---------: | :---------------: |
| Linux (`x86_64` and `aarch64`)                                       | ✓ (via [systemd]) |      ✓      |      Stable       |
| MacOS (`x86_64` and `aarch64`)                                       |         ✓         |             | Stable (see note) |
| [Valve Steam Deck][steam-deck] (SteamOS)                             |         ✓         |             |      Stable       |
| [Windows Subsystem for Linux][wsl] 2 (WSL2) (`x86_64` and `aarch64`) | ✓ (via [systemd]) |      ✓      |      Stable       |
| [Podman] Linux containers                                            | ✓ (via [systemd]) |      ✓      |      Stable       |
| [Docker] containers                                                  |                   |      ✓      |      Stable       |

## Install Nix

You can install Nix with the default [planner](#planners) and options by running this script:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install
```

To download a platform-specific installer binary yourself:

```shell
curl -sL -o nix-installer https://artifacts.nixos.org/nix-installer/nix-installer-x86_64-linux
chmod +x nix-installer
./nix-installer
```

This would install Nix on an `x86_64-linux` system but you can replace that with the system of your choice.

### Planners

The experimental Nix installer installs Nix by following a _plan_ made by a _planner_.
To review the available planners:

```shell
/nix/nix-installer install --help
```

Planners have their own options and defaults, sharing most of them in common.
To see the options for Linux, for example:

```shell
/nix/nix-installer install linux --help
```

You can configure planners using environment variables or command arguments:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  NIX_BUILD_GROUP_NAME=nixbuilder sh -s -- install --nix-build-group-id 4000

# Alternatively:

NIX_BUILD_GROUP_NAME=nixbuilder ./nix-installer install --nix-build-group-id 4000
```

See [Installer settings](#installer-settings) below for a full list of options.

### Troubleshooting

Having problems with the installer?
Consult our [troubleshooting guide](./docs/troubleshooting.md) to see if your problem is covered.

### Upgrading Nix

You can upgrade Nix by running:

```shell
sudo -i nix upgrade-nix
```

Alternatively, you can [uninstall](#uninstalling) and [reinstall](#install-nix) with a different version of the installer.

### Uninstalling

You can remove Nix installed by the experimental Nix installer by running:

```shell
/nix/nix-installer uninstall
```

### On GitLab

[GitLab CI][gitlab-ci] runners are typically [Docker] based and run as the `root` user.
This means that `systemd` is not present, so you need to pass the `--init none` option to the Linux planner.

On the default [GitLab] runners, you can install Nix using this configuration:

```yaml
test:
  script:
    - curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | sh -s -- install linux --no-confirm --init none
    - . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
    - nix run nixpkgs#hello
    - nix profile install nixpkgs#hello
    - hello
```

If you are using different runners, the above example may need to be adjusted.

### Without systemd (Linux only)

> [!WARNING]
> When `--init none` is used, _only_ `root` or users who can elevate to `root` privileges can run Nix:
>
> ```shell
> sudo -i nix run nixpkgs#hello
> ```

If you don't use [systemd], you can still install Nix by explicitly specifying the `linux` plan and `--init none`:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install linux --init none
```

### In a container

In [Docker]/[Podman] containers or [WSL2][wsl] instances where an init (like `systemd`) is not present, pass `--init none`.

For containers (without an init):

> [!WARNING]
> When `--init none` is used, _only_ `root` or users who can elevate to `root` privileges can run Nix:
>
> ```shell
> sudo -i nix run nixpkgs#hello
> ```

> [!WARNING]
> If you want to add a `flake.nix`, first declare a working directory (such as `/src`) in your `Dockerfile`.
> You cannot lock a flake placed at the docker image root (`/`) ([see details](https://github.com/DeterminateSystems/nix-installer/issues/1066)).
> You would get a `file '/dev/full' has an unsupported type` during the docker build.
>
> ```dockerfile
> # append this to the below dockerfiles
> WORKDIR /src
> # now flakes will work
> RUN nix flake init
> RUN nix flake lock
> ```

```dockerfile
# Dockerfile
FROM ubuntu:latest
RUN apt update -y
RUN apt install curl -y
RUN curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | sh -s -- install linux \
  --extra-conf "sandbox = false" \
  --init none \
  --no-confirm
ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"
RUN nix run nixpkgs#hello
```

```shell
docker build -t ubuntu-with-nix .
docker run --rm -ti ubuntu-with-nix
docker rmi ubuntu-with-nix
# or
podman build -t ubuntu-with-nix .
podman run --rm -ti ubuntu-with-nix
podman rmi ubuntu-with-nix
```

For containers with a [systemd] init:

```dockerfile
# Dockerfile
FROM ubuntu:latest
RUN apt update -y
RUN apt install curl systemd -y
RUN curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | sh -s -- install linux \
  --extra-conf "sandbox = false" \
  --no-start-daemon \
  --no-confirm
ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"
RUN nix run nixpkgs#hello
CMD [ "/bin/systemd" ]
```

```shell
podman build -t ubuntu-systemd-with-nix .
IMAGE=$(podman create ubuntu-systemd-with-nix)
CONTAINER=$(podman start $IMAGE)
podman exec -ti $CONTAINER /bin/bash
podman rm -f $CONTAINER
podman rmi $IMAGE
```

With some container tools, such as [Docker], you can omit `sandbox = false`.
Omitting this will negatively impact compatibility with container tools like [Podman].

### In GitHub Actions

[The nix installer action repository](https://github.com/NixOS/nix-installer-action/) provides a GitHub Action for installing Nix in CI workflows.
It uses this installer under the hood.

**Basic usage:**
```yaml
- uses: NixOS/nix-installer-action@main
```

**Install specific version:**
```yaml
- uses: NixOS/nix-installer-action@main
  with:
    installer-version: v3.11.3-experimental-prerelease
```

**No-init mode (for containers):**
```yaml
- uses: NixOS/nix-installer-action@main
  with:
    init: "no"
```

See the [action inputs](https://github.com/NixOS/nix-installer-action/tree/main?tab=readme-ov-file#inputs) for all available options.

### In WSL2

We **strongly recommend** first [enabling systemd][enabling-systemd] and then installing Nix as normal:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install
```

If [WSLg][wslg] is enabled, you can do things like open a Linux Firefox from Windows on Powershell:

```powershell
wsl nix run nixpkgs#firefox
```

To use some OpenGL applications, you can use [`nixGL`][nixgl] (note that some applications, such as `blender`, may not work):

```powershell
wsl nix run --impure github:guibou/nixGL nix run nixpkgs#obs-studio
```

If enabling systemd is not an option, pass `--init none` at the end of the command:

> [!WARNING]
> When `--init none` is used, _only_ `root` or users who can elevate to `root` privileges can run Nix:
>
> ```shell
> sudo -i nix run nixpkgs#hello
> ```

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install linux --init none
```

### Skip confirmation

If you'd like to bypass the confirmation step, you can apply the `--no-confirm` flag:

```shell
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer | \
  sh -s -- install --no-confirm
```

This is especially useful when using the installer in non-interactive scripts.

## Features

Existing Nix installation scripts do a good job but they are difficult to maintain.

Subtle differences in the shell implementations and tool used in the scripts make it difficult to make meaningful changes to the installer.

The experimental Nix installer has numerous advantages over these options:

- It keeps an installation _receipt_ for easy [uninstallation](#uninstalling)
- It uses [planners](#planners) to create appropriate install plans for complicated targets&mdash;plans that you can review prior to installation
- It enables you to perform a best-effort reversion in the facing of a failed install
- It improves installation performance by maximizing parallel operations
- It supports an expanded test suite including "curing" cases (compatibility with Nix already on the system)
- It supports SELinux and OSTree-based distributions without asking users to make compromises
- It operates as a single, static binary with external dependencies such as [OpenSSL], only calling existing system tools (like `useradd`) when necessary
- As a macOS remote build target, it ensures that Nix is present on the `PATH`

## Nix community involvement

It has been wonderful to collaborate with other participants in the [Nix Installer Working Group][wg] and members of the broader community.
The working group maintains a [foundation-owned fork of the installer][forked-installer].

## Quirks

While the experimental Nix Installer tries to provide a comprehensive and unquirky experience, there are unfortunately some issues that may require manual intervention or operator choices.
See [this document](./docs/quirks.md) for information on resolving these issues:

- [Using MacOS after removing Nix while nix-darwin was still installed, network requests fail](./docs/quirks.md#using-macos-after-removing-nix-while-nix-darwin-was-still-installed-network-requests-fail)

## Building a binary

See [this guide](./docs/building.md) for instructions on building and distributing the installer yourself.

## As a Rust library

The experimental Nix installer is available as a standard [Rust] library.
See [this guide](./docs/rust-library.md) for instructions on using the library in your own Rust code.

## Accessing other versions

You can pin to a specific version of the experimental Nix installer by modifying the download URL.
Here's an example:

```shell
VERSION="v0.6.0"
curl --proto '=https' --tlsv1.2 -sSf -L https://artifacts.nixos.org/nix-installer/tag/${VERSION}/nix-installer.sh | \
  sh -s -- install
```

To discover which versions are available, or download the binaries for any release, check the [Github Releases][releases].

You can download and use these releases directly.
Here's an example:

```shell
VERSION="v0.6.0"
ARCH="aarch64-linux"
curl -sSf -L https://github.com/NixOS/nix-installer/releases/download/${VERSION}/nix-installer-${ARCH} -o nix-installer
./nix-installer install
```

Each installer version has an [associated supported nix version](src/settings.rs)&mdash;if you pin the installer version, you'll also indirectly pin to the associated nix version.

You can also override the Nix version using `--nix-package-url` or `NIX_INSTALLER_NIX_PACKAGE_URL=` but doing this is not recommended since we haven't tested that combination.
Here are some example Nix package URLs, including the Nix version, OS, and architecture:

- https://releases.nixos.org/nix/nix-2.18.1/nix-2.18.1-x86_64-linux.tar.xz
- https://releases.nixos.org/nix/nix-2.18.1/nix-2.18.1-aarch64-darwin.tar.xz

## Installation differences

Differing from the upstream [Nix][upstream-nix] installer scripts:

* an installation receipt (for uninstalling) is stored at `/nix/receipt.json` as well as a copy of the install binary at `/nix/nix-installer`
* `ssl-cert-file` is set in `/etc/nix/nix.conf` if the `ssl-cert-file` argument is used.

## Installer settings

The experimental Nix installer provides a variety of configuration settings, some [general](#general-settings) and some on a per-command basis.
All settings are available via flags or via `NIX_INSTALLER_*` environment variables.

### General settings

These settings are available for all commands.

| Flag(s)            | Description                                                               | Default (if any) | Environment variable           |
| ------------------ | ------------------------------------------------------------------------- | ---------------- | ------------------------------ |
| `--log-directives` | Tracing directives delimited by comma                                     |                  | `NIX_INSTALLER_LOG_DIRECTIVES` |
| `--logger`         | Which logger to use (options are `compact`, `full`, `pretty`, and `json`) | `compact`        | `NIX_INSTALLER_LOGGER`         |
| `--verbose`        | Enable debug logs, (`-vv` for trace)                                      | `false`          | `NIX_INSTALLER_VERBOSITY`      |

### Installation (`nix-installer install`)

| Flag(s)                    | Description                                                                                        | Default (if any)                     | Environment variable                   |
| -------------------------- | -------------------------------------------------------------------------------------------------- | ------------------------------------ | -------------------------------------- |
| `--explain`                | Provide an explanation of the changes the installation process will make to your system            | `false`                              | `NIX_INSTALLER_EXPLAIN`                |
| `--extra-conf`             | Extra configuration lines for `/etc/nix.conf`                                                      |                                      | `NIX_INSTALLER_EXTRA_CONF`             |
| `--force`                  | Whether the installer should forcibly recreate files it finds existing                             | `false`                              | `NIX_INSTALLER_FORCE`                  |
| `--init`                   | Which init system to configure (if `--init none` Nix will be root-only)                            | `launchd` (macOS), `systemd` (Linux) | `NIX_INSTALLER_INIT`                   |
| `--nix-build-group-id`     | The Nix build group GID                                                                            | `350` (macOS), `30000` (Linux)       | `NIX_INSTALLER_NIX_BUILD_GROUP_ID`     |
| `--nix-build-group-name`   | The Nix build group name                                                                           | `nixbld`                             | `NIX_INSTALLER_NIX_BUILD_GROUP_NAME`   |
| `--nix-build-user-count`   | The number of build users to create                                                                | `32`                                 | `NIX_INSTALLER_NIX_BUILD_USER_COUNT`   |
| `--nix-build-user-id-base` | The Nix build user base UID (ascending) (NOTE: the first UID will be this base + 1)                | `350` (macOS), `30000` (Linux)       | `NIX_INSTALLER_NIX_BUILD_USER_ID_BASE` |
| `--nix-build-user-prefix`  | The Nix build user prefix (user numbers will be postfixed)                                         | `_nixbld` (macOS), `nixbld` (Linux)  | `NIX_INSTALLER_NIX_BUILD_USER_PREFIX`  |
| `--nix-package-url`        | The Nix package URL                                                                                |                                      | `NIX_INSTALLER_NIX_PACKAGE_URL`        |
| `--no-confirm`             | Run installation without requiring explicit user confirmation                                      | `false`                              | `NIX_INSTALLER_NO_CONFIRM`             |
| `--no-modify-profile`      | Modify the user profile to automatically load Nix.                                                 | `true`                               | `NIX_INSTALLER_MODIFY_PROFILE`         |
| `--proxy`                  | The proxy to use (if any); valid proxy bases are `https://$URL`, `http://$URL` and `socks5://$URL` |                                      | `NIX_INSTALLER_PROXY`                  |
| `--ssl-cert-file`          | An SSL cert to use (if any); used for fetching Nix and sets `ssl-cert-file` in `/etc/nix/nix.conf` |                                      | `NIX_INSTALLER_SSL_CERT_FILE`          |
| `--no-start-daemon`        | Start the daemon (if not `--init none`)                                                            | `true`                               | `NIX_INSTALLER_START_DAEMON`           |

You can also specify a planner with the first argument:

```shell
nix-installer install <plan>
```

Alternatively, you can use the `NIX_INSTALLER_PLAN` environment variable:

```shell
NIX_INSTALLER_PLAN=<plan> nix-installer install
```

### Uninstalling (`nix-installer uninstall`)

| Flag(s)        | Description                                                                             | Default (if any) | Environment variable       |
| -------------- | --------------------------------------------------------------------------------------- | ---------------- | -------------------------- |
| `--explain`    | Provide an explanation of the changes the installation process will make to your system | `false`          | `NIX_INSTALLER_EXPLAIN`    |
| `--no-confirm` | Run installation without requiring explicit user confirmation                           | `false`          | `NIX_INSTALLER_NO_CONFIRM` |

You can also specify an installation receipt as the first argument (the default is `/nix/receipt.json`):

```shell
nix-installer uninstall /path/to/receipt.json
```

### Planning (`nix-installer plan`)

| Flag(s)      | Description                                        | Default (if any) | Environment variable          |
| ------------ | -------------------------------------------------- | ---------------- | ----------------------------- |
| `--out-file` | Where to write the generated plan (in JSON format) | `/dev/stdout`    | `NIX_INSTALLER_PLAN_OUT_FILE` |

### Repairing (`nix-installer repair`)

| Flag(s)        | Description                                                   | Default (if any) | Environment variable       |
| -------------- | ------------------------------------------------------------- | ---------------- | -------------------------- |
| `--no-confirm` | Run installation without requiring explicit user confirmation | `false`          | `NIX_INSTALLER_NO_CONFIRM` |

### Self-test (`nix-installer self-test`)

`nix-installer self-test` only takes [general settings](#general-settings).

[actions]: https://github.com/features/actions
[docker]: https://docker.com
[enabling-systemd]: https://devblogs.microsoft.com/commandline/systemd-support-is-now-available-in-wsl/#how-can-you-get-systemd-on-your-machine
[flakes]: https://zero-to-nix.com/concepts/flakes
[forked-installer]: https://github.com/nixos/nix-installer
[gitlab]: https://gitlab.com
[gitlab-ci]: https://docs.gitlab.com/ee/ci
[nix]: https://nixos.org
[nixgl]: https://github.com/guibou/nixGL
[nixos]: https://zero-to-nix.com/concepts/nixos
[openssl]: https://openssl.org
[podman]: https://podman.io
[releases]: https://github.com/NixOS/nix-installer/releases
[rust]: https://rust-lang.org
[selinux]: https://selinuxproject.org
[steam-deck]: https://store.steampowered.com/steamdeck
[systemd]: https://systemd.io
[upstream-nix]: https://github.com/NixOS/nix
[wg]: https://discourse.nixos.org/t/nix-installer-workgroup/21495
[wsl]: https://learn.microsoft.com/en-us/windows/wsl/about
[wslg]: https://github.com/microsoft/wslg
