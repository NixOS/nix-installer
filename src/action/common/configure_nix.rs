use std::path::PathBuf;

use crate::{
    action::{
        base::SetupDefaultProfile,
        common::{ConfigureShellProfile, PlaceNixConfiguration},
        Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction,
    },
    planner::ShellProfileLocations,
    settings::{CommonSettings, SCRATCH_DIR},
};

use crate::action::common::SetupChannels;

use tracing::{span, Span};

/**
Configure Nix and start it
 */
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(tag = "action_name", rename = "configure_nix")]
pub struct ConfigureNix {
    setup_default_profile: StatefulAction<SetupDefaultProfile>,
    configure_shell_profile: Option<StatefulAction<ConfigureShellProfile>>,
    place_nix_configuration: Option<StatefulAction<PlaceNixConfiguration>>,
    setup_channels: Option<StatefulAction<SetupChannels>>,
}

impl ConfigureNix {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(
        shell_profile_locations: ShellProfileLocations,
        settings: &CommonSettings,
    ) -> Result<StatefulAction<Self>, ActionError> {
        let setup_default_profile =
            SetupDefaultProfile::plan(PathBuf::from(SCRATCH_DIR)).map_err(Self::error)?;

        let configure_shell_profile = if settings.modify_profile {
            Some(ConfigureShellProfile::plan(shell_profile_locations).map_err(Self::error)?)
        } else {
            None
        };

        let place_nix_configuration = if settings.skip_nix_conf {
            None
        } else {
            Some(
                PlaceNixConfiguration::plan(
                    settings.nix_build_group_name.clone(),
                    settings.ssl_cert_file.clone(),
                    settings.extra_conf.clone(),
                    settings.force,
                )
                .map_err(Self::error)?,
            )
        };

        let setup_channels = if settings.add_channel {
            Some(SetupChannels::plan().map_err(Self::error)?)
        } else {
            None
        };

        Ok(Self {
            place_nix_configuration,
            setup_default_profile,
            configure_shell_profile,
            setup_channels,
        }
        .into())
    }
}

#[typetag::serde(name = "configure_nix")]
impl Action for ConfigureNix {
    fn action_tag() -> ActionTag {
        ActionTag("configure_nix")
    }
    fn tracing_synopsis(&self) -> String {
        "Configure Nix".to_string()
    }

    fn tracing_span(&self) -> Span {
        span!(tracing::Level::DEBUG, "configure_nix",)
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        let Self {
            setup_default_profile,
            place_nix_configuration,
            configure_shell_profile,
            setup_channels,
        } = &self;

        let mut buf = setup_default_profile.describe_execute();
        if let Some(place_nix_configuration) = place_nix_configuration {
            buf.append(&mut place_nix_configuration.describe_execute());
        }
        if let Some(setup_channels) = setup_channels {
            buf.append(&mut setup_channels.describe_execute());
        }
        if let Some(configure_shell_profile) = configure_shell_profile {
            buf.append(&mut configure_shell_profile.describe_execute());
        }
        buf
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        let Self {
            setup_default_profile,
            place_nix_configuration,
            configure_shell_profile,
            setup_channels,
        } = self;

        let setup_default_profile_span = tracing::Span::current().clone();
        let _setup_channels_span = setup_channels
            .is_some()
            .then(|| setup_default_profile_span.clone());

        if let Some(place_nix_configuration) = place_nix_configuration {
            place_nix_configuration.try_execute().map_err(Self::error)?;
        }
        setup_default_profile.try_execute().map_err(Self::error)?;
        if let Some(configure_shell_profile) = configure_shell_profile {
            configure_shell_profile.try_execute().map_err(Self::error)?;
        }

        // Keep setup_channels outside try_join to avoid the error:
        // SQLite database '/nix/var/nix/db/db.sqlite' is busy
        // Presumably there are conflicts with nix commands run in
        // setup_default_profile.
        if let Some(setup_channels) = setup_channels {
            setup_channels.try_execute().map_err(Self::error)?;
        }

        Ok(())
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        let Self {
            setup_default_profile,
            place_nix_configuration,
            configure_shell_profile,
            setup_channels,
        } = &self;

        let mut buf = Vec::default();
        if let Some(configure_shell_profile) = configure_shell_profile {
            buf.append(&mut configure_shell_profile.describe_revert());
        }
        if let Some(place_nix_configuration) = place_nix_configuration {
            buf.append(&mut place_nix_configuration.describe_revert());
        }
        buf.append(&mut setup_default_profile.describe_revert());
        if let Some(setup_channels) = setup_channels {
            buf.append(&mut setup_channels.describe_revert());
        }

        buf
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        let mut errors = vec![];
        if let Some(configure_shell_profile) = &mut self.configure_shell_profile {
            if let Err(err) = configure_shell_profile.try_revert() {
                errors.push(err);
            }
        }
        if let Some(place_nix_configuration) = &mut self.place_nix_configuration {
            if let Err(err) = place_nix_configuration.try_revert() {
                errors.push(err);
            }
        }
        if let Err(err) = self.setup_default_profile.try_revert() {
            errors.push(err);
        }

        if let Some(setup_channels) = &mut self.setup_channels {
            if let Err(err) = setup_channels.try_revert() {
                errors.push(err);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors
                .into_iter()
                .next()
                .expect("Expected 1 len Vec to have at least 1 item"))
        } else {
            Err(Self::error(ActionErrorKind::MultipleChildren(errors)))
        }
    }
}
