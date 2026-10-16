use super::*;
use crate::browsers::{FirefoxDefault, Inventory};

fn profile(home: &std::path::Path, name: &str, files: &[&str]) -> Target {
    let database = home.join(name);
    fs::create_dir(&database).unwrap();
    for name in files {
        fs::write(database.join(name), b"synthetic data").unwrap();
    }
    Target {
        family: Family::Firefox,
        label: format!("Firefox — {name}"),
        database,
        create: false,
        firefox_default: Some(FirefoxDefault::Profile),
        installations: Vec::new(),
    }
}

#[test]
fn empty_and_partial_profiles_are_not_offered_but_initialized_secondary_profiles_remain() {
    let home = tempfile::tempdir().unwrap();
    let main = profile(home.path(), "main", &FILES);
    let secondary = profile(home.path(), "work", &FILES);
    let empty = profile(home.path(), "default", &["times.json"]);
    let partial = profile(home.path(), "partial", &["cert9.db"]);
    let inventory = Inventory {
        installed: vec!["Firefox".into()],
        targets: vec![
            main.clone(),
            empty.clone(),
            secondary.clone(),
            partial.clone(),
        ],
        notices: vec!["existing discovery notice".into()],
        ambiguous_firefox_profiles: false,
    };
    assert_eq!(
        serde_json::to_value(&inventory).unwrap()["targets"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let (targets, notices) = inventory.configuration_targets(None);
    assert_eq!(
        targets
            .iter()
            .map(|target| &target.database)
            .collect::<Vec<_>>(),
        [&main.database, &secondary.database]
    );
    assert_eq!(notices.len(), 3);
    assert!(notices[1].contains("Firefox — default"));
    assert!(notices[1].contains("Abra esse perfil"));
    assert!(notices[2].contains("Firefox — partial"));
    assert_eq!(fs::read_dir(&empty.database).unwrap().count(), 1);
    assert_eq!(
        fs::read(empty.database.join("times.json")).unwrap(),
        b"synthetic data"
    );
    assert_eq!(fs::read_dir(&partial.database).unwrap().count(), 1);
    assert!(!home.path().join(".local/state").exists());
}

#[test]
fn profile_becomes_selectable_after_browser_initializes_it_without_changing_discovery() {
    let home = tempfile::tempdir().unwrap();
    let target = profile(home.path(), "default", &["times.json"]);
    let mut notices = Vec::new();
    assert!(eligible(vec![target.clone()], &mut notices).is_empty());
    for name in FILES {
        fs::write(
            target.database.join(name),
            b"initialized by synthetic browser",
        )
        .unwrap();
    }
    notices.clear();
    assert_eq!(eligible(vec![target.clone()], &mut notices).len(), 1);
    assert!(notices.is_empty());
    assert_eq!(
        fs::read(target.database.join("times.json")).unwrap(),
        b"synthetic data"
    );
}

#[test]
fn chromium_creation_and_family_filter_are_preserved() {
    let home = tempfile::tempdir().unwrap();
    let empty = profile(home.path(), "firefox", &[]);
    let mut chromium = empty.clone();
    chromium.family = Family::Chromium;
    chromium.label = "Chromium".into();
    chromium.database = home.path().join("not-created");
    chromium.create = true;
    assert!(!needs_initialization(&chromium));
    let inventory = Inventory {
        installed: Vec::new(),
        targets: vec![empty, chromium],
        notices: Vec::new(),
        ambiguous_firefox_profiles: false,
    };
    let (targets, notices) = inventory.configuration_targets(Some(Family::Chromium));
    assert_eq!(targets.len(), 1);
    assert!(notices.is_empty());
    assert!(!targets[0].database.exists());
}

#[test]
fn symlinks_wrong_file_types_and_missing_profile_are_not_mislabeled_as_uninitialized() {
    let home = tempfile::tempdir().unwrap();
    let target = profile(home.path(), "unsafe", &[]);
    std::os::unix::fs::symlink(home.path().join("missing"), target.database.join("key4.db"))
        .unwrap();
    assert!(!needs_initialization(&target));
    assert_eq!(eligible(vec![target.clone()], &mut Vec::new()).len(), 1);
    fs::remove_file(target.database.join("key4.db")).unwrap();
    fs::create_dir(target.database.join("cert9.db")).unwrap();
    assert!(!needs_initialization(&target));
    let mut absent = target;
    absent.database = home.path().join("missing-profile");
    assert!(!needs_initialization(&absent));
}

#[test]
fn explicit_registration_of_uninitialized_profile_reports_action_without_lock_or_backup() {
    use crate::browsers::{
        register,
        report::{FailureKind, TargetResult, TargetStatus},
    };
    let home = tempfile::tempdir().unwrap();
    let target = profile(home.path(), "empty", &["times.json"]);
    let module = home.path().join("unused-provider.so");
    fs::write(&module, b"must never load NSS or this library").unwrap();
    let result = TargetResult::from_registration(
        target.clone(),
        register(&target, &module, home.path(), false),
    );
    assert_eq!(result.status, TargetStatus::NeedsAction);
    let failure = result.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::ProfileNotInitialized);
    assert!(failure.message.contains("Abra-o no navegador"));
    assert!(failure.recovery.is_none());
    assert_eq!(fs::read_dir(&target.database).unwrap().count(), 1);
    assert!(!home.path().join(".local/state").exists());
}
