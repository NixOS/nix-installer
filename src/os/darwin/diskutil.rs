use std::path::PathBuf;

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DiskUtilInfoOutput {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub parent_whole_disk: String,
    pub global_permissions_enabled: bool,
    pub mount_point: Option<PathBuf>,
}

impl DiskUtilInfoOutput {
    pub fn for_volume_name(volume_name: &str) -> Result<Self, crate::action::ActionErrorKind> {
        Self::for_volume_path(std::path::Path::new(volume_name))
    }

    pub fn for_volume_path(
        volume_path: &std::path::Path,
    ) -> Result<Self, crate::action::ActionErrorKind> {
        let buf = crate::execute_command(
            std::process::Command::new("/usr/sbin/diskutil")
                .args(["info", "-plist"])
                .arg(volume_path)
                .stdin(std::process::Stdio::null()),
        )?
        .stdout;

        Ok(plist::from_reader(std::io::Cursor::new(buf))?)
    }

    pub fn is_mounted(&self) -> bool {
        match self.mount_point {
            None => false,
            Some(ref mp) => !mp.as_os_str().is_empty(),
        }
    }
}

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DiskUtilApfsListOutput {
    pub containers: Vec<DiskUtilApfsContainer>,
}

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DiskUtilApfsContainer {
    pub volumes: Vec<DiskUtilApfsListVolume>,
}

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DiskUtilApfsListVolume {
    pub name: Option<String>,
    pub file_vault: Option<bool>,
}
