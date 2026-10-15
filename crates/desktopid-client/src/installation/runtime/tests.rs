use super::*;
use std::os::unix::fs::PermissionsExt;

fn native_file(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = fs::read("/usr/bin/true").unwrap();
    bytes.extend_from_slice(MARKER);
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn user(home: &Path) -> Runtime {
    let module = home.join(USER_MODULE);
    let helper = home.join(USER_HELPER);
    native_file(&module);
    native_file(&helper);
    Runtime { module, helper }
}

#[test]
fn old_rpm_is_ignored_and_does_not_hide_a_missing_native_user_installation() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    fs::create_dir(&home).unwrap();
    let old_module = temporary.path().join("old-system-module.so");
    fs::copy("/usr/bin/true", &old_module).unwrap();
    assert!(select_with(&home, &old_module, None, |_| true).is_err());
    let native = user(&home);
    assert_eq!(
        select_with(&home, &old_module, None, |_| true).unwrap(),
        native
    );
}

#[test]
fn a_native_helper_is_not_enough_to_select_a_legacy_system_module() {
    let temporary = tempfile::tempdir().unwrap();
    let native = user(temporary.path());
    let old_module = temporary.path().join("old-system-module.so");
    fs::copy("/usr/bin/true", &old_module).unwrap();
    assert_eq!(
        select_with(
            temporary.path(),
            &old_module,
            Some(&native.helper),
            native_artifact
        )
        .unwrap(),
        native
    );
}

#[test]
fn compatible_system_pair_keeps_rpm_precedence() {
    let temporary = tempfile::tempdir().unwrap();
    user(temporary.path());
    let module = temporary.path().join("native-system-module.so");
    let helper = temporary.path().join("native-system-helper");
    native_file(&module);
    native_file(&helper);
    assert_eq!(
        select_with(temporary.path(), &module, Some(&helper), native_artifact).unwrap(),
        Runtime { module, helper }
    );
}

#[test]
fn native_user_selection_rejects_mixed_old_helper_symlinks_and_writable_files() {
    let temporary = tempfile::tempdir().unwrap();
    let native = user(temporary.path());
    fs::copy("/usr/bin/true", &native.helper).unwrap();
    assert!(user_runtime(temporary.path()).is_err());
    native_file(&native.helper);
    fs::set_permissions(&native.module, fs::Permissions::from_mode(0o722)).unwrap();
    assert!(user_runtime(temporary.path()).is_err());
    fs::set_permissions(&native.module, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_file(&native.helper).unwrap();
    std::os::unix::fs::symlink("/usr/bin/true", &native.helper).unwrap();
    assert!(user_runtime(temporary.path()).is_err());
}

#[test]
fn native_user_selection_rejects_a_symlinked_runtime_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let outside = temporary.path().join("outside");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".local")).unwrap();
    user(&home);
    assert!(user_runtime(&home).is_err());
}

#[test]
fn runtime_identifier_is_found_across_read_buffer_boundaries_but_not_in_scripts() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("artifact");
    let mut bytes = b"\x7fELF".to_vec();
    bytes.resize(65536 + MARKER.len() / 2, 0);
    bytes.extend_from_slice(MARKER);
    fs::write(&path, bytes).unwrap();
    assert!(native_artifact(&path));
    fs::write(&path, [b"#!/bin/sh\n".as_slice(), MARKER].concat()).unwrap();
    assert!(!native_artifact(&path));
}

#[test]
fn markerless_and_non_hardlinked_executables_are_not_current_native_pairs() {
    let temporary = tempfile::tempdir().unwrap();
    let helper = temporary.path().join("helper");
    let launcher = temporary.path().join("launcher");
    fs::copy("/usr/bin/true", &helper).unwrap();
    fs::hard_link(&helper, &launcher).unwrap();
    assert!(!super::super::native_executable_pair(&helper, &launcher));
    native_file(&helper);
    assert!(super::super::native_executable_pair(&helper, &launcher));
    fs::remove_file(&launcher).unwrap();
    fs::copy(&helper, &launcher).unwrap();
    assert!(!super::super::native_executable_pair(&helper, &launcher));
}

#[test]
fn migration_accepts_only_our_obsolete_system_registration_and_current_user_pair() {
    let temporary = tempfile::tempdir().unwrap();
    let native = user(temporary.path());
    let system = Path::new(SYSTEM_MODULE);
    assert!(can_replace_with(
        temporary.path(),
        system,
        &native.module,
        None,
        |_| true
    ));
    assert!(!can_replace_with(
        temporary.path(),
        system,
        &native.module,
        Some(system),
        |_| true
    ));
    assert!(!can_replace_with(
        temporary.path(),
        system,
        &native.module,
        None,
        |_| false
    ));
    assert!(!can_replace_with(
        temporary.path(),
        Path::new("/usr/lib64/another-provider.so"),
        &native.module,
        None,
        |_| true
    ));
    assert!(!can_replace_with(
        temporary.path(),
        system,
        &temporary.path().join("custom.so"),
        None,
        |_| true
    ));
    fs::copy("/usr/bin/true", &native.helper).unwrap();
    assert!(!can_replace_with(
        temporary.path(),
        system,
        &native.module,
        None,
        |_| true
    ));
}
