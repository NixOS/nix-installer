//! The tag and the registry dispatch come from one declaration.

mod common;

use std::path::PathBuf;

use common::{CreateFile, CreateUser, test_registry};
use nix_installer_kernel::{
    Assessment, Executor, HostFacts, RevertPolicy, Session, StepDecl, StepRegistry,
};
use nix_installer_macros::step;

/// A declaration without a handler.
#[step(name = "kept_marker", category = "test", revert = "keep")]
struct KeptMarker {
    path: PathBuf,
}

fn file_spec(dir: &std::path::Path, i: u32) -> CreateFile {
    CreateFile {
        path: dir.join(format!("registry-{i}.conf")),
        content: format!("content {i}\n"),
        mode: 0o644,
    }
}

#[test]
fn meta_comes_from_single_declaration() {
    assert_eq!(CreateFile::META.name, "create_file");
    assert_eq!(CreateFile::META.category, "files");
    assert_eq!(CreateUser::META.name, "create_user");
    assert_eq!(CreateUser::META.category, "users");
}

#[test]
fn revert_keep_is_declared() {
    assert_eq!(KeptMarker::META.revert, RevertPolicy::Keep);
    assert_eq!(CreateFile::META.revert, RevertPolicy::Revert);
}

#[test]
fn serde_tag_key_is_action_name() {
    let dir = tempfile::tempdir().unwrap();
    let value = serde_json::to_value(file_spec(dir.path(), 1)).unwrap();
    assert_eq!(
        value["action_name"], "create_file",
        "receipts use the `action_name` key"
    );
}

#[test]
fn duplicate_registration_is_an_error() {
    let mut registry = StepRegistry::new();
    registry.register::<CreateFile>().unwrap();
    let err = registry.register::<CreateFile>().unwrap_err();
    assert_eq!(err.to_string(), "step `create_file` registered twice");
}

#[test]
fn unknown_name_is_not_found_and_bad_payloads_error() {
    let registry = test_registry();
    assert!(registry.get("no_such_step").is_none());
    assert!(
        registry
            .deserialize_name("no_such_step", serde_json::Value::Null)
            .is_none()
    );

    // A known name with a payload missing the spec's fields is an error
    // rather than a silent miss, since receipts are untrusted input.
    let bad = registry.deserialize_name(
        "create_file",
        serde_json::json!({"action_name": "create_file"}),
    );
    assert!(matches!(bad, Some(Err(_))));
}

#[test]
fn roundtrip_through_registry_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let registry = test_registry();

    let user = CreateUser {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    };
    let samples = vec![
        serde_json::to_value(file_spec(dir.path(), 2)).unwrap(),
        serde_json::to_value(&user).unwrap(),
    ];

    for value in samples {
        let name = value["action_name"]
            .as_str()
            .expect("tag present")
            .to_string();
        let erased = registry
            .deserialize_name(&name, value.clone())
            .expect("registered")
            .unwrap_or_else(|err| panic!("deserialize {name}: {err}"));
        assert_eq!(erased.name(), name);
        assert_eq!(erased.meta().name, name);
        assert_eq!(erased.to_value().unwrap(), value, "round-trip is lossless");
    }
}

#[test]
fn erased_dispatch_runs_the_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let registry = test_registry();
    let spec = file_spec(dir.path(), 3);
    let erased = registry
        .deserialize_name("create_file", serde_json::to_value(&spec).unwrap())
        .unwrap()
        .unwrap();

    let mut session = Session::new(HostFacts::fabricated(false), Executor::Real);
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Todo);
    erased.apply(&mut session).unwrap();
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Done);
    erased.revert(&mut session).unwrap();
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Todo);
}
