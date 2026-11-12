use nix::unistd::{Group, User, chown};
use tracing::{Span, span};

use std::{
    fs::Metadata,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use nix_installer_kernel::{
    ApplyGate, Assessment, Code, Executor, Failure, HostFacts, Outcome, Probe, RevertGate, Session, StepDecl,
    StepHandler, fs::atomic_write,
};
use nix_installer_macros::step;

use crate::{
    action::{Action, ActionDescription, ActionError, ActionErrorKind, ActionTag, StatefulAction},
    util::OnMissing,
};

/** Create a file at the given location with the provided `buf`,
optionally with an owning user, group, and mode.

If `force` is set, the file will always be overwritten (and deleted)
regardless of its presence prior to install.
 */
#[step(name = "create_file", category = "base")]
pub struct CreateFile {
    pub(crate) path: PathBuf,
    user: Option<String>,
    group: Option<String>,
    mode: Option<u32>,
    buf: String,
    force: bool,
}

enum Mismatch {
    NotFile,
    Mode { found: u32, planned: u32 },
    User { found: u32, planned: u32 },
    Group { found: u32, planned: u32 },
    Content,
}

impl Mismatch {
    fn to_action_kind(&self, path: &Path) -> ActionErrorKind {
        match self {
            Mismatch::NotFile => ActionErrorKind::PathWasNotFile(path.to_path_buf()),
            Mismatch::Mode { found, planned } => {
                ActionErrorKind::PathModeMismatch(path.to_path_buf(), *found, *planned)
            },
            Mismatch::User { found, planned } => {
                ActionErrorKind::PathUserMismatch(path.to_path_buf(), *found, *planned)
            },
            Mismatch::Group { found, planned } => {
                ActionErrorKind::PathGroupMismatch(path.to_path_buf(), *found, *planned)
            },
            Mismatch::Content => ActionErrorKind::DifferentContent(path.to_path_buf()),
        }
    }

    /// Only a content difference has its own code. Attribute conflicts share
    /// [`Code::PathConflict`].
    fn to_drift(&self, path: &Path) -> Assessment {
        let (remedy, detail) = match self {
            Mismatch::NotFile => (
                Code::PathConflict,
                format!("`{}` exists, but is not a file", path.display()),
            ),
            Mismatch::Mode { found, planned } => (
                Code::PathConflict,
                format!(
                    "`{}` exists with mode {found:o}, but {planned:o} was planned",
                    path.display()
                ),
            ),
            Mismatch::User { found, planned } => (
                Code::PathConflict,
                format!(
                    "`{}` exists with uid {found}, but {planned} was planned",
                    path.display()
                ),
            ),
            Mismatch::Group { found, planned } => (
                Code::PathConflict,
                format!(
                    "`{}` exists with gid {found}, but {planned} was planned",
                    path.display()
                ),
            ),
            Mismatch::Content => (
                Code::DifferentContent,
                format!(
                    "`{}` exists with different content than planned",
                    path.display()
                ),
            ),
        };
        Assessment::drift(remedy, detail)
    }
}

impl CreateFile {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn plan(
        path: impl AsRef<Path>,
        user: impl Into<Option<String>>,
        group: impl Into<Option<String>>,
        mode: impl Into<Option<u32>>,
        buf: String,
        force: bool,
    ) -> Result<StatefulAction<Self>, ActionError> {
        let this = Self {
            path: path.as_ref().to_path_buf(),
            user: user.into(),
            group: group.into(),
            mode: mode.into(),
            buf,
            force,
        };

        match std::fs::metadata(&this.path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(StatefulAction::uncompleted(this))
            },
            Err(err) => Err(Self::error(ActionErrorKind::GettingMetadata(
                this.path.clone(),
                err,
            ))),
            Ok(metadata) => match this.mismatch(&metadata)? {
                Some(mismatch) => Err(Self::error(mismatch.to_action_kind(&this.path))),
                None => {
                    tracing::debug!("Creating file `{}` already complete", this.path.display());
                    Ok(StatefulAction::completed(this))
                },
            },
        }
    }

    /// Checks type, mode, owner, group, then content, and returns the first
    /// difference.
    fn mismatch(&self, metadata: &Metadata) -> Result<Option<Mismatch>, ActionError> {
        if !metadata.is_file() {
            return Ok(Some(Mismatch::NotFile));
        }
        if let Some(planned) = self.mode {
            let found = metadata.permissions().mode() & 0o777;
            if found != planned {
                return Ok(Some(Mismatch::Mode { found, planned }));
            }
        }
        if let Some(user) = &self.user {
            let planned = Self::planned_uid(user)?;
            let found = metadata.uid();
            if found != planned {
                return Ok(Some(Mismatch::User { found, planned }));
            }
        }
        if let Some(group) = &self.group {
            let planned = Self::planned_gid(group)?;
            let found = metadata.gid();
            if found != planned {
                return Ok(Some(Mismatch::Group { found, planned }));
            }
        }
        let found = std::fs::read_to_string(&self.path)
            .map_err(|e| ActionErrorKind::Read(self.path.clone(), e))
            .map_err(Self::error)?;
        if found != self.buf {
            return Ok(Some(Mismatch::Content));
        }
        Ok(None)
    }

    /// Errors if `user` does not exist.
    fn planned_uid(user: &str) -> Result<u32, ActionError> {
        User::from_name(user)
            .map_err(|e| ActionErrorKind::GettingUserId(user.to_string(), e))
            .map_err(Self::error)?
            .ok_or_else(|| ActionErrorKind::NoUser(user.to_string()))
            .map_err(Self::error)
            .map(|user| user.uid.as_raw())
    }

    /// Errors if `group` does not exist.
    fn planned_gid(group: &str) -> Result<u32, ActionError> {
        Group::from_name(group)
            .map_err(|e| ActionErrorKind::GettingGroupId(group.to_string(), e))
            .map_err(Self::error)?
            .ok_or_else(|| ActionErrorKind::NoGroup(group.to_string()))
            .map_err(Self::error)
            .map(|group| group.gid.as_raw())
    }

    fn session() -> Session {
        Session::new(HostFacts::detect(), Executor::Real)
    }

    /// Content drift keeps its legacy error. Everything else is wrapped
    /// as is.
    fn to_action_error(&self, failure: Failure) -> ActionError {
        let kind = match failure.code {
            Code::DifferentContent => ActionErrorKind::DifferentContent(self.path.clone()),
            _ => ActionErrorKind::Custom(Box::new(failure)),
        };
        Self::error(kind)
    }

    /// `Indeterminate` has no legacy equivalent, so it surfaces as a
    /// transient failure.
    fn indeterminate_error(&self, probe: &Probe) -> ActionError {
        Self::error(ActionErrorKind::Custom(Box::new(Failure::new(
            Code::Transient,
            format!(
                "could not determine the state of `{}`: {}",
                self.path.display(),
                probe.describe()
            ),
        ))))
    }
}

impl StepHandler for CreateFile {
    /// A path that cannot be statted for a reason other than absence is
    /// `Indeterminate` and not an error, so a re-run can re-check it.
    fn assess(&self, _session: &Session) -> Result<Assessment, Failure> {
        match std::fs::metadata(&self.path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Assessment::Todo),
            Err(_) => Ok(Assessment::Indeterminate {
                probe: Probe::PathContent {
                    path: self.path.clone(),
                },
            }),
            Ok(metadata) => match self
                .mismatch(&metadata)
                .map_err(|err| Failure::new(Code::Io, err.to_string()))?
            {
                Some(mismatch) => Ok(mismatch.to_drift(&self.path)),
                None => Ok(Assessment::Done),
            },
        }
    }

    fn apply(&self, session: &mut Session) -> Result<Outcome, Failure> {
        if let ApplyGate::Settled(outcome) = self.assess(session)?.apply_gate()? {
            return Ok(outcome);
        }
        atomic_write(&self.path, self.buf.as_bytes(), self.mode).map_err(|err| {
            Failure::new(
                Code::Io,
                format!("writing `{}`: {err}", self.path.display()),
            )
        })?;
        self.chown()?;
        Ok(Outcome::Applied)
    }

    /// A drifted file is removed too, since refusing to uninstall over a
    /// user's edit would strand the machine.
    fn revert(&self, session: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(session)?.revert_gate() {
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::Revert | RevertGate::BestEffort { .. } | RevertGate::Unknown { .. } => {
                self.remove()?;
                Ok(Outcome::Applied)
            },
        }
    }
}

impl CreateFile {
    fn remove(&self) -> Result<(), Failure> {
        crate::util::remove_file(&self.path, OnMissing::Ignore).map_err(|err| {
            Failure::new(
                Code::Io,
                format!("removing `{}`: {err}", self.path.display()),
            )
        })
    }

    fn chown(&self) -> Result<(), Failure> {
        let uid = match &self.user {
            Some(user) => Some(
                User::from_name(user.as_str())
                    .map_err(|err| Failure::new(Code::Io, format!("getting user `{user}`: {err}")))?
                    .ok_or_else(|| Failure::new(Code::Io, format!("no such user `{user}`")))?
                    .uid,
            ),
            None => None,
        };
        let gid = match &self.group {
            Some(group) => Some(
                Group::from_name(group.as_str())
                    .map_err(|err| {
                        Failure::new(Code::Io, format!("getting group `{group}`: {err}"))
                    })?
                    .ok_or_else(|| Failure::new(Code::Io, format!("no such group `{group}`")))?
                    .gid,
            ),
            None => None,
        };
        if uid.is_some() || gid.is_some() {
            chown(&self.path, uid, gid).map_err(|err| {
                Failure::new(
                    Code::Io,
                    format!("chowning `{}`: {err}", self.path.display()),
                )
            })?;
        }
        Ok(())
    }
}

#[typetag::serde(name = "create_file")]
impl Action for CreateFile {
    fn action_tag() -> ActionTag {
        ActionTag(Self::META.name)
    }
    fn tracing_synopsis(&self) -> String {
        format!("Create or overwrite file `{}`", self.path.display())
    }

    fn tracing_span(&self) -> Span {
        let span = span!(
            tracing::Level::DEBUG,
            CreateFile::META.name,
            path = tracing::field::display(self.path.display()),
            user = self.user,
            group = self.group,
            mode = self
                .mode
                .map(|v| tracing::field::display(format!("{:#o}", v))),
            buf = tracing::field::Empty,
        );

        if tracing::enabled!(tracing::Level::TRACE) {
            span.record("buf", &self.buf);
        }
        span
    }

    fn execute_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(self.tracing_synopsis(), vec![])]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn execute(&mut self) -> Result<(), ActionError> {
        if tracing::enabled!(tracing::Level::TRACE) {
            let span = tracing::Span::current();
            span.record("buf", &self.buf);
        }

        let mut session = Self::session();
        match self.apply(&mut session) {
            Ok(Outcome::Applied) => Ok(()),
            Ok(Outcome::Indeterminate { probe }) => Err(self.indeterminate_error(&probe)),
            Err(failure) => Err(self.to_action_error(failure)),
        }
    }

    fn revert_description(&self) -> Vec<ActionDescription> {
        vec![ActionDescription::new(
            format!("Delete file `{}`", self.path.display()),
            vec![format!("Delete file `{}`", self.path.display())],
        )]
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn revert(&mut self) -> Result<(), ActionError> {
        let mut session = Self::session();
        StepHandler::revert(self, &mut session)
            .map(|_| ())
            .map_err(|failure| self.to_action_error(failure))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use color_eyre::eyre::eyre;
    use std::fs::write;

    #[test]
    fn creates_and_deletes_file() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir.path().join("creates_and_deletes_file");
        let mut action =
            CreateFile::plan(test_file.clone(), None, None, None, "Test".into(), false)?;

        action.try_execute()?;

        action.try_revert()?;

        assert!(!test_file.exists(), "File should have been deleted");

        Ok(())
    }

    #[test]
    fn creates_and_deletes_file_even_if_edited() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir
            .path()
            .join("creates_and_deletes_file_even_if_edited");
        let mut action =
            CreateFile::plan(test_file.clone(), None, None, None, "Test".into(), false)?;

        action.try_execute()?;

        write(test_file.as_path(), "More content")?;

        action.try_revert()?;

        assert!(!test_file.exists(), "File should have been deleted");

        Ok(())
    }

    #[test]
    fn recognizes_existing_exact_files_and_reverts_them() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir
            .path()
            .join("recognizes_existing_exact_files_and_reverts_them");

        let test_content = "Some content";
        write(test_file.as_path(), test_content)?;

        let mut action = CreateFile::plan(
            test_file.clone(),
            None,
            None,
            None,
            test_content.into(),
            false,
        )?;

        action.try_execute()?;

        action.try_revert()?;

        assert!(!test_file.exists(), "File should have been deleted");

        Ok(())
    }

    #[test]
    fn recognizes_existing_different_files_and_errors() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir
            .path()
            .join("recognizes_existing_different_files_and_errors");

        write(test_file.as_path(), "Some content")?;

        match CreateFile::plan(
            test_file.clone(),
            None,
            None,
            None,
            "Some different content".into(),
            false,
        ) {
            Err(error) => match error.kind() {
                ActionErrorKind::DifferentContent(path) => assert_eq!(path, test_file.as_path()),
                _ => {
                    return Err(eyre!(
                        "Should have returned an ActionErrorKind::Exists error"
                    ));
                },
            },
            _ => {
                return Err(eyre!(
                    "Should have returned an ActionErrorKind::Exists error"
                ));
            },
        };

        assert!(test_file.exists(), "File should have not been deleted");

        Ok(())
    }

    #[test]
    fn recognizes_wrong_mode_and_errors() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir.path().join("recognizes_wrong_mode_and_errors");
        let initial_mode = 0o777;
        let expected_mode = 0o000;

        write(test_file.as_path(), "Some content")?;
        std::fs::set_permissions(test_file.as_path(), PermissionsExt::from_mode(initial_mode))?;

        match CreateFile::plan(
            test_file.clone(),
            None,
            None,
            Some(expected_mode),
            "Some different content".into(),
            false,
        ) {
            Err(err) => match err.kind() {
                ActionErrorKind::PathModeMismatch(path, got, expected) => {
                    assert_eq!(path, test_file.as_path());
                    assert_eq!(*expected, expected_mode);
                    assert_eq!(*got, initial_mode);
                },
                _ => {
                    return Err(eyre!(
                        "Should have returned an ActionErrorKind::PathModeMismatch error"
                    ));
                },
            },
            _ => {
                return Err(eyre!(
                    "Should have returned an ActionErrorKind::PathModeMismatch error"
                ));
            },
        }

        assert!(test_file.exists(), "File should have not been deleted");

        Ok(())
    }

    #[test]
    fn recognizes_correct_mode() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir.path().join("recognizes_correct_mode");
        let initial_mode = 0o777;

        write(test_file.as_path(), "Some content")?;
        std::fs::set_permissions(test_file.as_path(), PermissionsExt::from_mode(initial_mode))?;

        let mut action = CreateFile::plan(
            test_file.clone(),
            None,
            None,
            Some(initial_mode),
            "Some content".into(),
            false,
        )?;

        action.try_execute()?;

        action.try_revert()?;

        assert!(!test_file.exists(), "File should have been deleted");

        Ok(())
    }

    #[test]
    fn errors_on_dir() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;

        match CreateFile::plan(
            temp_dir.path(),
            None,
            None,
            None,
            "Some different content".into(),
            false,
        ) {
            Err(err) => match err.kind() {
                ActionErrorKind::PathWasNotFile(path) => assert_eq!(path, temp_dir.path()),
                _ => {
                    return Err(eyre!(
                        "Should have returned an ActionErrorKind::PathWasNotFile error"
                    ));
                },
            },
            _ => {
                return Err(eyre!(
                    "Should have returned an ActionErrorKind::PathWasNotFile error"
                ));
            },
        }

        Ok(())
    }

    #[test]
    fn execute_refuses_a_file_that_drifted_after_planning() -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let test_file = temp_dir
            .path()
            .join("execute_refuses_a_file_that_drifted_after_planning");
        let mut action =
            CreateFile::plan(test_file.clone(), None, None, None, "Test".into(), false)?;

        write(test_file.as_path(), "Someone else got here first")?;

        match action.try_execute() {
            Err(error) => match error.kind() {
                ActionErrorKind::DifferentContent(path) => assert_eq!(path, test_file.as_path()),
                _ => return Err(eyre!("drift must surface as DifferentContent")),
            },
            Ok(()) => return Err(eyre!("execute must refuse a drifted file")),
        }
        assert_eq!(
            std::fs::read_to_string(&test_file)?,
            "Someone else got here first",
            "the drifted file must be untouched"
        );

        Ok(())
    }
}
