use std::collections::HashMap;

use crate::execute_command;

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("Profile plist parsing error: {0}")]
    Parse(#[from] plist::Error),

    #[error("Profile discovery error: {0}")]
    ProfileListing(#[from] crate::ActionErrorKind),
}

/// Extract just the XML plist content from the output.
///
/// `/usr/bin/profiles` may emit non-XML text before or after the plist
/// (e.g. "There are no configuration profiles installed"). The position
/// varies across macOS versions. We extract the `<?xml`..`</plist>` range
/// to avoid plist parse failures from surrounding plain text.
fn extract_plist(buf: &[u8]) -> &[u8] {
    const START_TAG: &[u8] = b"<?xml";
    const END_TAG: &[u8] = b"</plist>";

    let start = buf.windows(START_TAG.len()).position(|w| w == START_TAG);
    let end = buf
        .windows(END_TAG.len())
        .rposition(|w| w == END_TAG)
        .map(|pos| pos + END_TAG.len());

    match (start, end) {
        (Some(s), Some(e)) if s < e => &buf[s..e],
        _ => buf,
    }
}

pub fn load() -> Result<Policies, LoadError> {
    let buf = execute_command(
        std::process::Command::new("/usr/bin/profiles")
            // "prints all configuration profiles to console"
            .arg("-P")
            // "path to output XML plist file (for -P, -L, -C).  Use 'stdout' to send information to the console."
            // NOTE(grahamc): `stdout` doesn't output XML formatting, but `stdout-xml` does
            .args(["-o", "stdout-xml"])
            .stdin(std::process::Stdio::null()),
    )?
    .stdout;

    parse(&buf)
}

pub fn parse(buf: &[u8]) -> Result<Policies, LoadError> {
    let xml = extract_plist(buf);
    Ok(plist::from_reader(std::io::Cursor::new(xml))?)
}

pub type Policies = HashMap<Target, Vec<Profile>>;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Target {
    #[serde(rename(deserialize = "_computerlevel"))]
    Computer,
    #[serde(untagged)]
    User(String),
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct Profile {
    pub profile_description: Option<String>,
    pub profile_display_name: Option<String>,
    pub profile_identifier: Option<String>,
    pub profile_install_date: Option<String>,
    #[serde(rename = "ProfileUUID")]
    pub profile_uuid: Option<String>,
    pub profile_version: Option<usize>,

    #[serde(default)]
    pub profile_items: Vec<ProfileItem>,
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "PayloadType", content = "PayloadContent")]
pub enum ProfileItem {
    #[serde(rename = "com.apple.systemuiserver")]
    SystemUIServer(SystemUIServer),

    #[serde(untagged)]
    Unknown(UnknownProfileItem),
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct UnknownProfileItem {
    payload_type: Option<String>,
    payload_content: Option<plist::Value>,
}

impl std::cmp::Eq for UnknownProfileItem {}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct SystemUIServer {
    pub mount_controls: Option<MountControls>,
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct MountControls {
    #[serde(default)]
    pub harddisk_internal: Vec<HardDiskInternalOpts>,
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HardDiskInternalOpts {
    Authenticate,
    ReadOnly,
    Deny,
    Eject,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_parse_blocking_policy() {
        let parsed: Policies = plist::from_reader(std::io::Cursor::new(include_str!(
            "./profile.sample.block.plist"
        )))
        .unwrap();
        assert_eq!(
            Policies::from([(
                Target::User("foo".into()),
                vec![Profile {
                    profile_description: Some("The description".into()),
                    profile_display_name: Some("Don't allow mounting internal devices".into()),
                    profile_identifier: Some(
                        "MyProfile.6F6670A3-65AC-4EA4-8665-91F8FCE289AB".into()
                    ),
                    profile_install_date: Some("2024-04-22 14:12:42 +0000".into()),
                    profile_uuid: Some("6F6670A3-65AC-4EA4-8665-91F8FCE289AB".into()),
                    profile_version: Some(1),
                    profile_items: vec![ProfileItem::SystemUIServer(SystemUIServer {
                        mount_controls: Some(MountControls {
                            harddisk_internal: vec![HardDiskInternalOpts::Deny],
                        })
                    })],
                }]
            )]),
            parsed
        );
    }

    /// Regression test: `/usr/bin/profiles -P -o stdout-xml` emits
    /// "There are no configuration profiles installed" as plain text
    /// surrounding the plist XML when no profiles exist. The position
    /// varies across macOS versions (before or after the XML). This
    /// caused a `Parse(UnexpectedXmlCharactersExpectedElement)` error
    /// that made `check_suis()` skip the SystemUIServer policy check.
    #[test]
    fn parse_empty_profiles_with_trailing_text() {
        let raw = include_bytes!("./profile.sample.empty-with-trailing-text.plist");
        assert!(
            plist::from_reader::<_, Policies>(std::io::Cursor::new(raw)).is_err(),
            "raw input should fail to parse without extraction"
        );
        let parsed = parse(raw).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_empty_profiles_with_leading_text() {
        let raw = include_bytes!("./profile.sample.empty-with-leading-text.plist");
        assert!(
            plist::from_reader::<_, Policies>(std::io::Cursor::new(raw)).is_err(),
            "raw input should fail to parse without extraction"
        );
        let parsed = parse(raw).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn try_parse_unknown() {
        let parsed: Policies = plist::from_reader(std::io::Cursor::new(include_str!(
            "./profile.sample.unknown.plist"
        )))
        .unwrap();

        assert_eq!(
            Policies::from([(
                Target::Computer,
                vec![Profile {
                    profile_description: Some("".into()),
                    profile_display_name: Some(
                        "macOS Software Update Policy: Mandatory Minor Upgrades".into()
                    ),
                    profile_identifier: Some("com.example".into()),
                    profile_install_date: Some("2024-04-22 00:00:00 +0000".into()),
                    profile_uuid: Some("F7972F85-2A4D-4609-A4BB-02CB0C34A3F8".into()),
                    profile_version: Some(1),
                    profile_items: vec![ProfileItem::Unknown(UnknownProfileItem {
                        payload_type: Some("com.apple.SoftwareUpdate".into()),
                        payload_content: Some(plist::Value::Dictionary({
                            let mut dict = plist::dictionary::Dictionary::new();
                            dict.insert("AllowPreReleaseInstallation".into(), false.into());
                            dict.insert("AutomaticCheckEnabled".into(), true.into());
                            dict
                        }))
                    })],
                }]
            )]),
            parsed
        );
    }
}
