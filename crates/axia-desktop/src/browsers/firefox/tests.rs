use super::super::{Inventory, discover_at, setup_with};
use super::*;

fn fixture(ini: &str, directories: &[&str]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("config/mozilla/firefox");
    fs::create_dir_all(&root).unwrap();
    for directory in directories {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("profiles.ini"), ini).unwrap();
    fs::create_dir(home.path().join("bin")).unwrap();
    super::super::test_browser(&home.path().join("bin/firefox"));
    home
}

fn discover(home: &Path) -> Inventory {
    discover_at(
        home,
        &home.join("config"),
        &home.join("data"),
        &home.join("bin"),
    )
    .unwrap()
}

#[test]
fn installation_default_does_not_hide_general_default_or_secondary_profiles() {
    let home = fixture(
        "[Profile0]\nName=Antigo\nPath=old\nDefault=1\n[Profile1]\nName=Principal\nPath=main\n[Profile2]\nName=Trabalho\nPath=work\n[InstallA]\nDefault=main\n",
        &["old", "main", "work"],
    );
    // A Chromium target must not affect selection of the Firefox default.
    super::super::test_browser(&home.path().join("bin/chromium"));
    let inventory = discover(home.path());
    assert_eq!(inventory.targets.len(), 4);
    assert_eq!(
        inventory.targets[0].firefox_default,
        Some(FirefoxDefault::Installation)
    );
    assert_eq!(
        inventory.targets[1].firefox_default,
        Some(FirefoxDefault::Profile)
    );
    assert_eq!(inventory.targets[2].firefox_default, None);
    assert!(inventory.targets[0].label.contains("padrão de instalação"));
    assert!(inventory.targets[1].label.contains("padrão geral"));
    assert!(inventory.targets[2].label.contains("Trabalho"));
    let json = serde_json::to_value(&inventory).unwrap();
    assert_eq!(json["targets"][0]["firefox_default"], "installation");
    assert_eq!(json["targets"][1]["firefox_default"], "profile");
    assert!(json["targets"][2].get("firefox_default").is_none());
    for field in ["family", "label", "database", "create"] {
        assert!(json["targets"][0].get(field).is_some());
    }
    assert_eq!(
        inventory.firefox_target().unwrap().database,
        home.path().join("config/mozilla/firefox/main")
    );
}

#[test]
fn unique_general_default_is_selected_without_hiding_other_profiles() {
    let home = fixture(
        "[Profile0]\nName=Principal\nPath=main\nDefault=1\n[Profile1]\nName=Trabalho\nPath=work\n",
        &["main", "work"],
    );
    let inventory = discover(home.path());
    assert_eq!(inventory.targets.len(), 2);
    assert_eq!(
        inventory.firefox_target().unwrap().database,
        home.path().join("config/mozilla/firefox/main")
    );
}

#[test]
fn absent_or_ambiguous_defaults_require_selection_unless_only_one_profile_exists() {
    for (ini, count) in [
        ("[Profile0]\nPath=main\n[Profile1]\nPath=work\n", 2),
        (
            "[Profile0]\nPath=main\nDefault=1\n[Profile1]\nPath=work\nDefault=1\n",
            2,
        ),
        (
            "[Profile0]\nPath=main\nDefault=1\n[Profile1]\nPath=work\n[InstallA]\nDefault=main\n[InstallB]\nDefault=work\n",
            2,
        ),
        ("[Profile0]\nPath=missing\n", 0),
    ] {
        let home = fixture(ini, &["main", "work"]);
        let inventory = discover(home.path());
        assert_eq!(inventory.targets.len(), count);
        assert!(
            inventory
                .firefox_target()
                .unwrap_err()
                .to_string()
                .contains("explicitamente")
        );
    }
    let home = fixture("[Profile0]\nPath=main\n", &["main"]);
    assert!(
        discover(home.path())
            .firefox_target()
            .unwrap()
            .database
            .ends_with("main")
    );
}

#[test]
fn canonical_duplicates_across_roots_merge_names_and_default_metadata() {
    let home = fixture(
        "[Profile0]\nName=Principal\nPath=shared\n[Profile1]\nName=Alias\nPath=alias\nDefault=1\n[InstallA]\nDefault=alias\n",
        &["shared"],
    );
    let shared = home.path().join("config/mozilla/firefox/shared");
    std::os::unix::fs::symlink(&shared, home.path().join("config/mozilla/firefox/alias")).unwrap();
    let legacy = home.path().join(".mozilla/firefox");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("profiles.ini"),
        format!(
            "[Profile0]\nName=Outro nome\nPath={}\nIsRelative=0\n[InstallB]\nDefault={}\n",
            shared.display(),
            shared.display()
        ),
    )
    .unwrap();
    let inventory = discover(home.path());
    assert_eq!(inventory.targets.len(), 1);
    assert_eq!(
        inventory.targets[0].label,
        "Firefox — Principal (padrão de instalação)"
    );
    assert_eq!(
        inventory.firefox_target().unwrap().database,
        shared.canonicalize().unwrap()
    );
}

#[test]
fn same_root_via_symlink_is_not_enumerated_twice() {
    let home = fixture(
        "[Profile0]\nName=Principal\nPath=main\n[Profile1]\nName=Trabalho\nPath=work\n",
        &["main", "work"],
    );
    fs::create_dir(home.path().join(".mozilla")).unwrap();
    std::os::unix::fs::symlink(
        home.path().join("config/mozilla/firefox"),
        home.path().join(".mozilla/firefox"),
    )
    .unwrap();
    assert_eq!(discover(home.path()).targets.len(), 2);
}

#[test]
fn identical_names_and_directory_basenames_have_distinct_local_choices() {
    let ini =
        "[Profile0]\nName=Trabalho\nPath=first/same\n[Profile1]\nName=Trabalho\nPath=second/same\n";
    let home = fixture(ini, &["first/same", "second/same"]);
    let legacy = home.path().join(".mozilla/firefox");
    fs::create_dir_all(legacy.join("first/same")).unwrap();
    fs::create_dir_all(legacy.join("second/same")).unwrap();
    fs::write(legacy.join("profiles.ini"), ini).unwrap();
    let inventory = discover(home.path());
    assert_eq!(inventory.targets.len(), 4);
    let labels: BTreeSet<_> = inventory
        .targets
        .iter()
        .map(|target| target.selection_label(home.path()))
        .collect();
    assert_eq!(labels.len(), 4);
    for target in &inventory.targets {
        assert_eq!(target.label, "Firefox — Trabalho");
        let label = target.selection_label(home.path());
        assert!(label.contains("\n~/"));
        assert!(!label.contains(home.path().to_str().unwrap()));
    }
    assert!(inventory.firefox_target().is_err());
}

#[test]
fn relative_absolute_missing_and_empty_paths_never_create_profiles() {
    let home = fixture("", &["relative"]);
    let root = home.path().join("config/mozilla/firefox");
    let absolute = home.path().join("absolute");
    fs::create_dir(&absolute).unwrap();
    fs::write(root.join("regular-file"), b"not a directory").unwrap();
    let ini = format!(
        "[Profile0]\nPath=relative\nIsRelative=1\n[Profile1]\nPath={}\nIsRelative=0\n[Profile2]\nPath=relative\nIsRelative=0\n[Profile3]\nPath=missing\n[Profile4]\nPath=\n[Profile5]\nPath=regular-file\n[InstallA]\nDefault=\n[InstallB]\nDefault=absent\n",
        absolute.display()
    );
    fs::write(root.join("profiles.ini"), &ini).unwrap();
    let inventory = discover(home.path());
    let paths: BTreeSet<_> = inventory
        .targets
        .iter()
        .map(|target| target.database.clone())
        .collect();
    assert_eq!(
        paths,
        [
            root.join("relative").canonicalize().unwrap(),
            absolute.canonicalize().unwrap()
        ]
        .into_iter()
        .collect()
    );
    assert!(!root.join("missing").exists());
    assert!(!root.join("absent").exists());
    assert_eq!(fs::read_to_string(root.join("profiles.ini")).unwrap(), ini);
    assert!(!home.path().join(".mozilla").exists());
}

#[test]
fn install_only_entry_gets_its_name_from_a_profile_in_another_root() {
    let home = fixture("[InstallA]\nDefault=main\n", &["main"]);
    let main = home.path().join("config/mozilla/firefox/main");
    let legacy = home.path().join(".mozilla/firefox");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(
        legacy.join("profiles.ini"),
        format!(
            "[Profile0]\nName=Principal\nIsRelative=0\nPath={}\n",
            main.display()
        ),
    )
    .unwrap();
    let inventory = discover(home.path());
    assert_eq!(inventory.targets.len(), 1);
    assert_eq!(
        inventory.targets[0].label,
        "Firefox — Principal (padrão de instalação)"
    );
}

#[test]
fn cancelling_preserves_both_profiles_and_confirming_only_passes_selected_secondary() {
    let home = fixture(
        "[Profile0]\nName=Principal\nPath=main\n[Profile1]\nName=Trabalho\nPath=work\n[Profile2]\nName=Sem uso\nPath=unused\nDefault=1\n[InstallA]\nDefault=main\n",
        &["main", "work", "unused"],
    );
    let root = home.path().join("config/mozilla/firefox");
    fs::write(root.join("unused/times.json"), b"preserve unused profile").unwrap();
    assert_eq!(discover(home.path()).targets.len(), 3);
    for directory in ["main", "work"] {
        for name in super::super::FILES {
            fs::write(root.join(directory).join(name), b"preserve profile").unwrap();
        }
    }
    let store = crate::storage::Store::new(home.path().join("identity")).unwrap();
    let mut config = store
        .initialize("https://example.invalid", "laboratory")
        .unwrap();
    config.desktop_code = Some("synthetic-code".into());
    store.save_registration(&config).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../desktopid-client/tests/fixtures/python-vectors.json"
    ))
    .unwrap();
    store
        .save_wallet(&[crate::client::WalletCertificate {
            key_name: "LAB-SERIAL;LAB-ISSUER".into(),
            certificate: openssl::x509::X509::from_pem(
                fixture["certificate_pem"].as_str().unwrap().as_bytes(),
            )
            .unwrap(),
        }])
        .unwrap();
    let wallet_before = fs::read(store.path().join("wallet.json")).unwrap();
    let prepare = || Ok((home.path().to_owned(), discover(home.path())));
    assert_eq!(
        setup_with(
            &store,
            None,
            prepare,
            |text, choices| {
                assert_eq!(choices.len(), 2);
                assert!(text.contains("Um navegador pode ter mais de um perfil"));
                assert!(text.contains("Sem uso (padrão geral): perfil ainda não preparado"));
                assert!(choices.iter().all(|choice| !choice.contains("Sem uso")));
                None
            },
            |_, _| panic!("cancel must not configure any profile")
        )
        .unwrap()
        .status,
        super::super::SetupStatus::Cancelled
    );
    for directory in ["main", "work"] {
        for name in super::super::FILES {
            assert_eq!(
                fs::read(root.join(directory).join(name)).unwrap(),
                b"preserve profile"
            );
        }
        assert_eq!(fs::read_dir(root.join(directory)).unwrap().count(), 3);
    }
    setup_with(
        &store,
        None,
        prepare,
        |_, choices| {
            Some(vec![
                choices
                    .iter()
                    .position(|label| label.ends_with("/work"))
                    .unwrap(),
            ])
        },
        |selected, _| {
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].database, root.join("work"));
            fs::write(
                selected[0].database.join("pkcs11.txt"),
                b"selected profile only",
            )?;
            Ok(super::super::SetupReport::from_results(Vec::new()))
        },
    )
    .unwrap();
    for name in super::super::FILES {
        assert_eq!(
            fs::read(root.join("main").join(name)).unwrap(),
            b"preserve profile"
        );
    }
    assert_eq!(
        fs::read(root.join("work/pkcs11.txt")).unwrap(),
        b"selected profile only"
    );
    assert_eq!(
        fs::read(root.join("unused/times.json")).unwrap(),
        b"preserve unused profile"
    );
    assert_eq!(fs::read_dir(root.join("unused")).unwrap().count(), 1);
    // With no initialized profile, return before selection/module/NSS work.
    for directory in ["main", "work"] {
        for name in super::super::FILES {
            fs::remove_file(root.join(directory).join(name)).unwrap();
        }
    }
    let absent = setup_with(
        &store,
        None,
        prepare,
        |_, _| panic!("uninitialized profiles must not be offered"),
        |_, _| panic!("must not select runtime or open NSS"),
    )
    .unwrap();
    assert_eq!(absent.status, super::super::SetupStatus::NoTargets);
    assert_eq!(absent.notices.len(), 3);
    assert_eq!(
        fs::read(store.path().join("wallet.json")).unwrap(),
        wallet_before
    );
    assert!(!home.path().join(".local/state").exists());
}
