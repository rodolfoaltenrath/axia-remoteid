use super::*;

fn laboratory_module(home: &Path) -> PathBuf {
    let module = home.join("laboratory-provider.so");
    fs::write(&module, b"not loaded by these guard tests").unwrap();
    fs::set_permissions(&module, fs::Permissions::from_mode(0o600)).unwrap();
    module
}

#[test]
fn running_chromium_refuses_before_creating_any_database_or_backup() {
    let home = tempfile::tempdir().unwrap();
    let target = Target {
        family: Family::Chromium,
        label: "Laboratory Chromium".into(),
        database: home.path().join(".pki/nssdb"),
        create: true,
    };
    let module = laboratory_module(home.path());
    let error = register_with(&target, &module, home.path(), false, || Ok(true))
        .err()
        .expect("running browser must prevent registration");
    assert!(error.to_string().contains("Feche os navegadores"));
    assert!(!home.path().join(".pki").exists());
    assert!(!home.path().join(".local").exists());
}

#[test]
fn unreadable_process_inventory_preserves_existing_database_and_permissions() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("database");
    fs::create_dir(&database).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o750)).unwrap();
    for name in FILES {
        fs::write(database.join(name), b"preserve this laboratory database").unwrap();
    }
    let target = Target {
        family: Family::Chromium,
        label: "Laboratory Chromium".into(),
        database: database.clone(),
        create: false,
    };
    let module = laboratory_module(home.path());
    assert!(
        register_with(&target, &module, home.path(), false, || {
            anyhow::bail!("laboratory procfs unavailable")
        })
        .is_err()
    );
    assert_eq!(fs::metadata(&database).unwrap().mode() & 0o777, 0o750);
    assert!(!database.join(".axia-integration.lock").exists());
    for name in FILES {
        assert_eq!(
            fs::read(database.join(name)).unwrap(),
            b"preserve this laboratory database"
        );
    }
}

#[test]
fn browser_starting_after_backup_is_detected_before_nss_can_write() {
    let home = tempfile::tempdir().unwrap();
    let target = Target {
        family: Family::Chromium,
        label: "Laboratory Chromium".into(),
        database: home.path().join(".pki/nssdb"),
        create: true,
    };
    let module = laboratory_module(home.path());
    let mut checks = 0;
    let error = register_with(&target, &module, home.path(), false, || {
        checks += 1;
        Ok(checks == 2)
    })
    .err()
    .expect("browser starting before NSS initialization must prevent registration");
    assert_eq!(checks, 2);
    assert!(error.to_string().contains("Feche os navegadores"));
    assert!(target.database.join(".axia-integration.lock").is_file());
    for name in FILES {
        assert!(
            !target.database.join(name).exists(),
            "NSS opened despite browser startup: {name}"
        );
    }
}

#[test]
fn browser_starting_during_nss_initialization_suspends_mutation_and_rollback() {
    let home = tempfile::tempdir().unwrap();
    let target = Target {
        family: Family::Chromium,
        label: "Laboratory Chromium".into(),
        database: home.path().join(".pki/nssdb"),
        create: true,
    };
    let module = laboratory_module(home.path());
    let mut checks = 0;
    let error = register_with(&target, &module, home.path(), false, || {
        checks += 1;
        Ok(checks >= 3)
    })
    .err()
    .expect("browser starting during NSS initialization must prevent module registration");
    assert_eq!(checks, 4);
    assert!(
        error
            .to_string()
            .contains("Restauração automática suspensa")
    );
    assert!(target.database.join("cert9.db").exists());
    assert!(registered_library(&target.database).unwrap().is_none());
    let backup_root = home
        .path()
        .join(".local/state/desktopid-linux/browser-backups");
    assert_eq!(fs::read_dir(backup_root).unwrap().count(), 1);
}

#[test]
fn rollback_never_overwrites_database_when_browser_is_running_or_scan_fails() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("database");
    fs::create_dir(&database).unwrap();
    fs::write(database.join("pkcs11.txt"), b"old snapshot").unwrap();
    let saved = backup(&database, home.path()).unwrap();
    fs::write(database.join("pkcs11.txt"), b"live browser contents").unwrap();
    let error = restore_closed(&database, &saved, Family::Chromium, &mut || Ok(true)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Restauração automática suspensa")
    );
    assert!(
        restore_closed(&database, &saved, Family::Chromium, &mut || {
            anyhow::bail!("laboratory scan failed")
        })
        .is_err()
    );
    assert_eq!(
        fs::read(database.join("pkcs11.txt")).unwrap(),
        b"live browser contents"
    );
    assert_eq!(fs::read(saved.join("pkcs11.txt")).unwrap(), b"old snapshot");
    restore_closed(&database, &saved, Family::Chromium, &mut || Ok(false)).unwrap();
    assert_eq!(
        fs::read(database.join("pkcs11.txt")).unwrap(),
        b"old snapshot"
    );
}

#[test]
fn firefox_uses_its_own_profile_lock_without_scanning_chromium_processes() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("database");
    fs::create_dir(&database).unwrap();
    fs::write(database.join("pkcs11.txt"), b"old snapshot").unwrap();
    let saved = backup(&database, home.path()).unwrap();
    fs::write(database.join("pkcs11.txt"), b"changed").unwrap();
    let mut must_not_scan = || panic!("Firefox must not scan unrelated Chromium processes");
    require_closed(Family::Firefox, &mut must_not_scan).unwrap();
    restore_closed(&database, &saved, Family::Firefox, &mut must_not_scan).unwrap();
    assert_eq!(
        fs::read(database.join("pkcs11.txt")).unwrap(),
        b"old snapshot"
    );
}
