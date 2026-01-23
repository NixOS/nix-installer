/*! Configurable knobs and their related errors
*/
use std::{collections::HashMap, path::PathBuf, str::FromStr};

#[cfg(feature = "cli")]
use clap::{
    ArgAction,
    error::{ContextKind, ContextValue},
};
use url::Url;

pub const SCRATCH_DIR: &str = "/nix/temp-install-dir";

pub const DEFAULT_NIX_BUILD_USER_GROUP_NAME: &str = "nixbld";

/// The embedded Nix tarball (zstd compressed)
pub const EMBEDDED_NIX_TARBALL: &[u8] = include_bytes!(concat!(env!("NIX_TARBALL_PATH")));

/// The store path of the nix package in the embedded tarball
pub const NIX_STORE_PATH: &str = env!("NIX_STORE_PATH");

/// The store path of the nss-cacert package in the embedded tarball
pub const NSS_CACERT_STORE_PATH: &str = env!("NSS_CACERT_STORE_PATH");

/// The version of Nix embedded in this installer
pub const NIX_VERSION: &str = env!("NIX_VERSION");

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum InitSystem {
    None,
    Systemd,
    Launchd,
}

impl std::fmt::Display for InitSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitSystem::None => write!(f, "none"),
            InitSystem::Systemd => write!(f, "systemd"),
            InitSystem::Launchd => write!(f, "launchd"),
        }
    }
}

/** Common settings used by all [`BuiltinPlanner`](crate::planner::BuiltinPlanner)s

Settings which only apply to certain [`Planner`](crate::planner::Planner)s should be located in the planner.

*/
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct CommonSettings {
    /// Modify the user profile to automatically load Nix
    #[cfg_attr(
        feature = "cli",
        clap(
            action(ArgAction::SetFalse),
            default_value = "true",
            global = true,
            env = "NIX_INSTALLER_MODIFY_PROFILE",
            long = "no-modify-profile"
        )
    )]
    pub modify_profile: bool,

    /// The Nix build group name
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            default_value = crate::settings::DEFAULT_NIX_BUILD_USER_GROUP_NAME,
            env = "NIX_INSTALLER_NIX_BUILD_GROUP_NAME",
            global = true
        )
    )]
    pub nix_build_group_name: String,

    /// The Nix build group GID
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_NIX_BUILD_GROUP_ID", global = true)
    )]
    #[cfg_attr(
        all(feature = "cli"),
        clap(default_value_t = default_nix_build_group_id())
    )]
    pub nix_build_group_id: u32,

    /// The Nix build user prefix (user numbers will be postfixed)
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_NIX_BUILD_USER_PREFIX", global = true)
    )]
    #[cfg_attr(
        all(target_os = "macos", feature = "cli"),
        clap(default_value = "_nixbld")
    )]
    #[cfg_attr(
        all(target_os = "linux", feature = "cli"),
        clap(default_value = "nixbld")
    )]
    pub nix_build_user_prefix: String,

    /// The number of build users to create
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            alias = "daemon-user-count",
            env = "NIX_INSTALLER_NIX_BUILD_USER_COUNT",
            global = true
        )
    )]
    #[cfg_attr(all(target_os = "macos", feature = "cli"), clap(default_value = "32"))]
    #[cfg_attr(all(target_os = "linux", feature = "cli"), clap(default_value = "32"))]
    pub nix_build_user_count: u32,

    /// The Nix build user base UID (ascending)
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_NIX_BUILD_USER_ID_BASE", global = true)
    )]
    #[cfg_attr(
        all(target_os = "macos", feature = "cli"),
        doc = "Service users on Mac should be between 200-400"
    )]
    #[cfg_attr(
        all(feature = "cli"),
        clap(default_value_t = default_nix_build_user_id_base())
    )]
    pub nix_build_user_id_base: u32,

    /// An SSL cert file to use; sets `ssl-cert-file` in `/etc/nix/nix.conf`
    #[cfg_attr(
        feature = "cli",
        clap(long, env = "NIX_INSTALLER_SSL_CERT_FILE", global = true)
    )]
    pub ssl_cert_file: Option<PathBuf>,

    /// Extra configuration lines for `/etc/nix.conf`
    #[cfg_attr(feature = "cli", clap(long, action = ArgAction::Append, num_args = 0.., env = "NIX_INSTALLER_EXTRA_CONF", global = true))]
    pub extra_conf: Vec<UrlOrPathOrString>,

    /// If `nix-installer` should forcibly recreate files it finds existing
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            action(ArgAction::SetTrue),
            default_value = "false",
            global = true,
            env = "NIX_INSTALLER_FORCE"
        )
    )]
    pub force: bool,

    /// If `nix-installer` should skip creating `/etc/nix/nix.conf`
    #[cfg_attr(
        feature = "cli",
        clap(
            long,
            action(ArgAction::SetTrue),
            default_value = "false",
            global = true,
            env = "NIX_INSTALLER_SKIP_NIX_CONF",
            conflicts_with = "extra_conf",
        )
    )]
    pub skip_nix_conf: bool,

    /// Whether to setup system channels
    #[cfg_attr(
        feature = "cli",
        clap(
            action(ArgAction::SetTrue),
            default_value = "false",
            global = true,
            env = "NIX_INSTALLER_ADD_CHANNEL",
            long("add-channel"),
        )
    )]
    pub add_channel: bool,
}

pub(crate) fn default_nix_build_user_id_base() -> u32 {
    use target_lexicon::OperatingSystem;

    match OperatingSystem::host() {
        OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => 350,
        _ => 30_000,
    }
}

pub(crate) fn default_nix_build_group_id() -> u32 {
    use target_lexicon::OperatingSystem;

    match OperatingSystem::host() {
        OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => 350,
        _ => 30_000,
    }
}

impl CommonSettings {
    /// The default settings for the given Architecture & Operating System
    pub fn default() -> Result<Self, InstallSettingsError> {
        let nix_build_user_prefix;

        use target_lexicon::{Architecture, OperatingSystem};
        match (Architecture::host(), OperatingSystem::host()) {
            (Architecture::X86_64, OperatingSystem::Linux) => {
                nix_build_user_prefix = "nixbld";
            },
            (Architecture::X86_32(_), OperatingSystem::Linux) => {
                nix_build_user_prefix = "nixbld";
            },
            (Architecture::Aarch64(_), OperatingSystem::Linux) => {
                nix_build_user_prefix = "nixbld";
            },
            (Architecture::X86_64, OperatingSystem::MacOSX(_))
            | (Architecture::X86_64, OperatingSystem::Darwin(_)) => {
                nix_build_user_prefix = "_nixbld";
            },
            (Architecture::Aarch64(_), OperatingSystem::MacOSX(_))
            | (Architecture::Aarch64(_), OperatingSystem::Darwin(_)) => {
                nix_build_user_prefix = "_nixbld";
            },
            _ => {
                return Err(InstallSettingsError::UnsupportedArchitecture(
                    target_lexicon::HOST,
                ));
            },
        };

        Ok(Self {
            modify_profile: true,
            nix_build_group_name: String::from(crate::settings::DEFAULT_NIX_BUILD_USER_GROUP_NAME),
            nix_build_group_id: default_nix_build_group_id(),
            nix_build_user_id_base: default_nix_build_user_id_base(),
            nix_build_user_count: 32,
            nix_build_user_prefix: nix_build_user_prefix.to_string(),
            ssl_cert_file: None,
            extra_conf: Default::default(),
            force: false,
            skip_nix_conf: false,
            add_channel: false,
        })
    }

    /// A listing of the settings, suitable for [`Planner::settings`](crate::planner::Planner::settings)
    pub fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let Self {
            modify_profile,
            nix_build_group_name,
            nix_build_group_id,
            nix_build_user_prefix,
            nix_build_user_id_base,
            nix_build_user_count,
            ssl_cert_file,
            extra_conf,
            force,
            skip_nix_conf,
            add_channel,
        } = self;
        let mut map = HashMap::default();

        map.insert(
            "modify_profile".into(),
            serde_json::to_value(modify_profile)?,
        );
        map.insert(
            "nix_build_group_name".into(),
            serde_json::to_value(nix_build_group_name)?,
        );
        map.insert(
            "nix_build_group_id".into(),
            serde_json::to_value(nix_build_group_id)?,
        );
        map.insert(
            "nix_build_user_prefix".into(),
            serde_json::to_value(nix_build_user_prefix)?,
        );
        map.insert(
            "nix_build_user_id_base".into(),
            serde_json::to_value(nix_build_user_id_base)?,
        );
        map.insert(
            "nix_build_user_count".into(),
            serde_json::to_value(nix_build_user_count)?,
        );
        map.insert("ssl_cert_file".into(), serde_json::to_value(ssl_cert_file)?);
        map.insert("extra_conf".into(), serde_json::to_value(extra_conf)?);
        map.insert("force".into(), serde_json::to_value(force)?);
        map.insert("skip_nix_conf".into(), serde_json::to_value(skip_nix_conf)?);

        map.insert("add_channel".into(), serde_json::to_value(add_channel)?);

        Ok(map)
    }
}

fn linux_detect_systemd_started() -> bool {
    use std::process::Stdio;

    let mut started = false;
    if std::path::Path::new("/run/systemd/system").exists() {
        started = std::process::Command::new("systemctl")
            .arg("status")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .map(|exit| exit.success())
            .unwrap_or(false)
    }

    // TODO: Other inits
    started
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[cfg_attr(feature = "cli", derive(clap::Parser))]
pub struct InitSettings {
    /// Which init system to configure (if `--init none` Nix will be root-only)
    #[cfg_attr(feature = "cli", clap(value_parser, long, env = "NIX_INSTALLER_INIT",))]
    #[cfg_attr(
        all(target_os = "macos", feature = "cli"),
        clap(default_value_t = InitSystem::Launchd)
    )]
    #[cfg_attr(
        all(target_os = "linux", feature = "cli"),
        clap(default_value_t = InitSystem::Systemd)
    )]
    pub init: InitSystem,

    /// Start the daemon (if not `--init none`)
    #[cfg_attr(
        feature = "cli",
        clap(
            value_parser,
            long,
            action(ArgAction::SetFalse),
            env = "NIX_INSTALLER_START_DAEMON",
            default_value_t = true,
            long = "no-start-daemon"
        )
    )]
    pub start_daemon: bool,
}

impl InitSettings {
    /// The default settings for the given Architecture & Operating System
    pub fn default() -> Result<Self, InstallSettingsError> {
        use target_lexicon::{Architecture, OperatingSystem};
        let (init, start_daemon) = match (Architecture::host(), OperatingSystem::host()) {
            (Architecture::X86_64, OperatingSystem::Linux) => {
                (InitSystem::Systemd, linux_detect_systemd_started())
            },
            (Architecture::X86_32(_), OperatingSystem::Linux) => {
                (InitSystem::Systemd, linux_detect_systemd_started())
            },
            (Architecture::Aarch64(_), OperatingSystem::Linux) => {
                (InitSystem::Systemd, linux_detect_systemd_started())
            },
            (Architecture::X86_64, OperatingSystem::MacOSX(_))
            | (Architecture::X86_64, OperatingSystem::Darwin(_)) => (InitSystem::Launchd, true),
            (Architecture::Aarch64(_), OperatingSystem::MacOSX(_))
            | (Architecture::Aarch64(_), OperatingSystem::Darwin(_)) => (InitSystem::Launchd, true),
            _ => {
                return Err(InstallSettingsError::UnsupportedArchitecture(
                    target_lexicon::HOST,
                ));
            },
        };

        Ok(Self { init, start_daemon })
    }

    /// A listing of the settings, suitable for [`Planner::settings`](crate::planner::Planner::settings)
    pub fn settings(&self) -> Result<HashMap<String, serde_json::Value>, InstallSettingsError> {
        let Self { init, start_daemon } = self;
        let mut map = HashMap::default();

        map.insert("init".into(), serde_json::to_value(init)?);
        map.insert("start_daemon".into(), serde_json::to_value(start_daemon)?);
        Ok(map)
    }

    /// Which init system to configure
    pub fn init(&mut self, init: InitSystem) -> &mut Self {
        self.init = init;
        self
    }

    /// Start the daemon (if one is configured)
    pub fn start_daemon(&mut self, toggle: bool) -> &mut Self {
        self.start_daemon = toggle;
        self
    }
}

/// An error originating from a [`Planner::settings`](crate::planner::Planner::settings)
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum InstallSettingsError {
    /// `nix-installer` does not support the architecture right now
    #[error("`nix-installer` does not support the `{0}` architecture right now")]
    UnsupportedArchitecture(target_lexicon::Triple),
    /// Parsing URL
    #[error("Parsing URL")]
    Parse(
        #[source]
        #[from]
        url::ParseError,
    ),
    /// JSON serialization or deserialization error
    #[error("JSON serialization or deserialization error")]
    SerdeJson(
        #[source]
        #[from]
        serde_json::Error,
    ),
    #[error("No supported init system found")]
    InitNotSupported,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Clone)]
pub enum UrlOrPathOrString {
    Url(Url),
    Path(PathBuf),
    String(String),
}

impl FromStr for UrlOrPathOrString {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match Url::parse(s) {
            Ok(url) => Ok(UrlOrPathOrString::Url(url)),
            Err(url::ParseError::RelativeUrlWithoutBase) => {
                // This is most likely a relative path (`./boop` or `boop`)
                // or an absolute path (`/boop`)
                //
                // So we'll see if such a path exists, and if so, use it
                let path = PathBuf::from(s);
                if path.exists() {
                    Ok(UrlOrPathOrString::Path(path))
                } else {
                    // The path doesn't exist, so the user is providing us with a string
                    Ok(UrlOrPathOrString::String(s.into()))
                }
            },
            Err(e) => Err(e),
        }
    }
}

#[cfg(feature = "cli")]
impl clap::builder::TypedValueParser for UrlOrPathOrString {
    type Value = UrlOrPathOrString;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value_str = value.to_str().ok_or_else(|| {
            let mut err = clap::Error::new(clap::error::ErrorKind::InvalidValue);
            err.insert(
                ContextKind::InvalidValue,
                ContextValue::String(format!("`{value:?}` not a UTF-8 string")),
            );
            err
        })?;
        match UrlOrPathOrString::from_str(value_str) {
            Ok(v) => Ok(v),
            Err(from_str_error) => {
                let mut err = clap::Error::new(clap::error::ErrorKind::InvalidValue).with_cmd(cmd);
                err.insert(
                    clap::error::ContextKind::Custom,
                    clap::error::ContextValue::String(from_str_error.to_string()),
                );
                Err(err)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FromStr, PathBuf, Url, UrlOrPathOrString};

    #[test]
    fn url_or_path_or_string_parses() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            UrlOrPathOrString::from_str("https://boop.bleat")?,
            UrlOrPathOrString::Url(Url::from_str("https://boop.bleat")?),
        );
        assert_eq!(
            UrlOrPathOrString::from_str("file:///boop/bleat")?,
            UrlOrPathOrString::Url(Url::from_str("file:///boop/bleat")?),
        );
        // The file *must* exist!
        assert_eq!(
            UrlOrPathOrString::from_str(file!())?,
            UrlOrPathOrString::Path(PathBuf::from_str(file!())?),
        );
        assert_eq!(
            UrlOrPathOrString::from_str("Boop")?,
            UrlOrPathOrString::String(String::from("Boop")),
        );
        Ok(())
    }
}
