use super::super::{Family, discover_in, test_browser};
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for directory in ["bin", "config", "data/applications", "system-applications"] {
        fs::create_dir_all(home.path().join(directory)).unwrap();
    }
    home
}

fn search(home: &Path) -> Search {
    Search {
        bins: vec![home.join("bin")],
        desktops: vec![
            home.join("data/applications"),
            home.join("system-applications"),
        ],
    }
}

fn inventory(home: &Path, search: &Search) -> super::super::Inventory {
    discover_in(home, &home.join("config"), &home.join("data"), search).unwrap()
}

fn profile(home: &Path, root: &str) -> PathBuf {
    let root = home.join(root);
    fs::create_dir_all(root.join("main")).unwrap();
    fs::create_dir_all(root.join("work")).unwrap();
    fs::write(root.join("profiles.ini"), "[Profile0]\nName=Principal\nPath=main\n[Profile1]\nName=Trabalho\nPath=work\n[InstallA]\nDefault=main\n").unwrap();
    root
}

fn desktop(home: &Path, id: &str, exec: &str) {
    fs::write(
        home.join("data/applications").join(id),
        format!("[Desktop Entry]\nType=Application\nName=Synthetic\nExec={exec}\n"),
    )
    .unwrap();
}

#[test]
fn native_librewolf_in_user_path_only_uses_its_own_xdg_profiles() {
    let home = fixture();
    test_browser(&home.path().join("bin/librewolf"));
    let root = profile(home.path(), "config/librewolf/librewolf");
    profile(home.path(), "config/mozilla/firefox");
    let before = fs::read(root.join("profiles.ini")).unwrap();
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["LibreWolf"]);
    assert_eq!(result.targets.len(), 2);
    for target in &result.targets {
        assert!(target.database.starts_with(&root));
        assert!(target.label.starts_with("LibreWolf —"));
        assert_eq!(target.installations[0].id, "librewolf");
        assert_eq!(
            target.installations[0].executables,
            [home.path().join("bin/librewolf")]
        );
    }
    assert!(result.firefox_target().is_err());
    assert_eq!(fs::read(root.join("profiles.ini")).unwrap(), before);
    assert!(!home.path().join(".pki").exists());
}

#[test]
fn firefox_and_librewolf_keep_identical_profile_names_and_cli_defaults_separate() {
    let home = fixture();
    for name in ["firefox", "librewolf"] {
        test_browser(&home.path().join("bin").join(name));
    }
    let firefox = profile(home.path(), "config/mozilla/firefox");
    let librewolf = profile(home.path(), "config/librewolf/librewolf");
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["Firefox", "LibreWolf"]);
    assert_eq!(result.targets.len(), 4);
    assert!(result.targets[0].label.starts_with("Firefox — Principal"));
    assert!(result.targets[2].label.starts_with("LibreWolf — Principal"));
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["targets"][2]["installations"][0]["id"], "librewolf");
    assert_eq!(
        json["targets"][2]["database"],
        librewolf.join("main").to_str().unwrap()
    );
    assert_eq!(
        result.firefox_target().unwrap().database,
        firefox.join("main")
    );
}

#[test]
fn librewolf_legacy_root_takes_precedence_even_when_it_has_no_profiles() {
    for populated in [true, false] {
        let home = fixture();
        test_browser(&home.path().join("bin/librewolf"));
        profile(home.path(), "config/librewolf/librewolf");
        let legacy = home.path().join(".librewolf");
        if populated {
            profile(home.path(), ".librewolf");
        } else {
            fs::create_dir(&legacy).unwrap();
        }
        let result = inventory(home.path(), &search(home.path()));
        if populated {
            assert_eq!(result.targets.len(), 2);
            assert!(
                result
                    .targets
                    .iter()
                    .all(|target| target.database.starts_with(&legacy))
            );
        } else {
            assert!(result.targets.is_empty());
            assert!(
                result
                    .notices
                    .iter()
                    .any(|notice| notice.contains("Abra e feche o LibreWolf"))
            );
            assert_eq!(fs::read_dir(legacy).unwrap().count(), 0);
        }
    }
}

#[test]
fn desktop_finds_quoted_absolute_executable_and_wayland_env_without_running_it() {
    let home = fixture();
    let location = home.path().join("opt/Libre Wolf");
    fs::create_dir_all(&location).unwrap();
    let binary = location.join("librewolf");
    let marker = home.path().join("MUST-NOT-RUN");
    fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf ran > '{}'\nexec /native/librewolf \"$@\"\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    profile(home.path(), "config/librewolf/librewolf");
    desktop(
        home.path(),
        "librewolf.desktop",
        &format!(
            "env GDK_BACKEND=wayland MOZ_ENABLE_WAYLAND=1 \"{}\" %u",
            binary.display()
        ),
    );
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["LibreWolf"]);
    assert_eq!(result.targets.len(), 2);
    assert_eq!(result.targets[0].installations[0].executables, [binary]);
    assert!(!marker.exists());
}

#[test]
fn duplicate_path_directories_symlinks_and_desktops_do_not_duplicate_targets() {
    let home = fixture();
    let binary = home.path().join("bin/librewolf");
    test_browser(&binary);
    profile(home.path(), "config/librewolf/librewolf");
    symlink(home.path().join("bin"), home.path().join("alias-bin")).unwrap();
    desktop(
        home.path(),
        "librewolf.desktop",
        &format!("{} %u", binary.display()),
    );
    desktop(
        home.path(),
        "io.gitlab.librewolf-community.desktop",
        &format!("{} %u", binary.display()),
    );
    let mut search = search(home.path());
    search
        .bins
        .extend([home.path().join("alias-bin"), home.path().join("bin")]);
    let result = inventory(home.path(), &search);
    assert_eq!(result.installed, ["LibreWolf"]);
    assert_eq!(result.targets.len(), 2);
    assert_eq!(result.targets[0].installations[0].executables, [binary]);
}

#[test]
fn another_products_executable_or_application_metadata_is_not_mislabeled() {
    let home = fixture();
    let actual = home.path().join("opt/firefox");
    fs::create_dir_all(&actual).unwrap();
    test_browser(&actual.join("firefox"));
    symlink(actual.join("firefox"), home.path().join("bin/firefox")).unwrap();
    symlink(actual.join("firefox"), home.path().join("bin/librewolf")).unwrap();
    profile(home.path(), "config/mozilla/firefox");
    profile(home.path(), "config/librewolf/librewolf");
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["Firefox"]);
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("outro navegador"))
    );
    fs::remove_file(home.path().join("bin/librewolf")).unwrap();
    test_browser(&actual.join("librewolf"));
    fs::write(
        actual.join("application.ini"),
        "[App]\nName=Firefox\nRemotingName=firefox\n[Other]\nRepeated=1\nRepeated=1\n",
    )
    .unwrap();
    symlink(actual.join("librewolf"), home.path().join("bin/librewolf")).unwrap();
    desktop(
        home.path(),
        "librewolf.desktop",
        &format!("{} %u", actual.join("librewolf").display()),
    );
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["Firefox"]);
    assert!(
        result
            .targets
            .iter()
            .all(|target| target.installations[0].id == "firefox")
    );
}

#[test]
fn literal_wrapper_forwarding_to_other_browser_is_not_associated() {
    let home = fixture();
    let binary = home.path().join("bin/librewolf");
    fs::write(&binary, "#!/bin/sh\nexec /usr/bin/firefox \"$@\"\n").unwrap();
    fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
    profile(home.path(), "config/librewolf/librewolf");
    let result = inventory(home.path(), &search(home.path()));
    assert!(result.targets.is_empty());
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("outro navegador"))
    );
}

#[test]
fn leftover_profiles_nonexecutables_and_dangling_launchers_do_not_prove_installation() {
    let home = fixture();
    profile(home.path(), "config/librewolf/librewolf");
    let binary = home.path().join("bin/librewolf");
    for state in 0..3 {
        if state == 1 {
            fs::write(&binary, b"\x7fELFnot executable").unwrap();
        }
        if state == 2 {
            fs::remove_file(&binary).unwrap();
            symlink("/missing/browser", &binary).unwrap();
        }
        let result = inventory(home.path(), &search(home.path()));
        assert!(result.installed.is_empty());
        assert!(result.targets.is_empty());
        assert!(
            result
                .notices
                .iter()
                .any(|notice| notice.contains("não instala navegadores"))
        );
    }
}

#[test]
fn native_executable_without_profile_only_produces_preparation_notice() {
    let home = fixture();
    test_browser(&home.path().join("bin/librewolf"));
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.installed, ["LibreWolf"]);
    assert!(result.targets.is_empty());
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("Abra e feche"))
    );
    assert!(!home.path().join("config/librewolf").exists());
}

#[test]
fn flatpak_desktop_and_wrapper_never_configure_host_profiles() {
    let home = fixture();
    profile(home.path(), "config/librewolf/librewolf");
    let binary = home.path().join("bin/librewolf");
    fs::write(
        &binary,
        "#!/bin/sh\nexec /usr/bin/flatpak run io.gitlab.librewolf-community\n",
    )
    .unwrap();
    fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
    desktop(
        home.path(),
        "io.gitlab.librewolf-community.desktop",
        "/usr/bin/flatpak run io.gitlab.librewolf-community %u",
    );
    fs::create_dir_all(home.path().join(".var/app/io.gitlab.librewolf-community")).unwrap();
    let result = inventory(home.path(), &search(home.path()));
    assert!(result.installed.is_empty());
    assert!(result.targets.is_empty());
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("Flatpak"))
    );
    assert!(!home.path().join(".pki").exists());
}

#[test]
fn snap_wrappers_and_renamed_appimages_are_not_treated_as_native_browsers() {
    for appimage in [false, true] {
        let home = fixture();
        profile(home.path(), "config/mozilla/firefox");
        let binary = home.path().join("bin/firefox");
        if appimage {
            let mut data = b"\x7fELFfake browser header".to_vec();
            data[8..11].copy_from_slice(b"AI\x02");
            fs::write(&binary, data).unwrap();
        } else {
            fs::write(&binary, "#!/bin/sh\nexec snap run firefox\n").unwrap();
        }
        fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            inventory(home.path(), &search(home.path()))
                .targets
                .is_empty()
        );
    }
}

#[test]
fn arbitrary_shells_profile_overrides_and_home_overrides_are_not_executed_or_associated() {
    let home = fixture();
    let marker = home.path().join("SECRET-LAB-MARKER");
    let binary_dir = home.path().join("opt/librewolf");
    fs::create_dir_all(&binary_dir).unwrap();
    let binary = binary_dir.join("librewolf");
    test_browser(&binary);
    profile(home.path(), "config/librewolf/librewolf");
    for exec in [
        format!("sh -c 'touch {}'", marker.display()),
        format!(
            "env HOME={} {} %u",
            home.path().join("another-home").display(),
            binary.display()
        ),
        format!("{} --profile /another/profile %u", binary.display()),
        format!("{} --user-data-dir=/another/profile %u", binary.display()),
        "\"unfinished quotation".to_owned(),
        format!("{} %INVALID", binary.display()),
        format!("{} -Pother-profile", binary.display()),
    ] {
        desktop(home.path(), "librewolf.desktop", &exec);
        let result = inventory(home.path(), &search(home.path()));
        assert!(result.targets.is_empty());
        assert!(!marker.exists());
        assert!(
            result
                .notices
                .iter()
                .all(|notice| !notice.contains("SECRET-LAB")
                    && !notice.contains(home.path().to_str().unwrap()))
        );
    }
}

#[test]
fn profile_shared_by_different_products_is_omitted_instead_of_assigned_arbitrarily() {
    let home = fixture();
    for name in ["firefox", "librewolf"] {
        test_browser(&home.path().join("bin").join(name));
    }
    let firefox = profile(home.path(), "config/mozilla/firefox");
    let root = home.path().join("config/librewolf/librewolf");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("profiles.ini"),
        format!(
            "[Profile0]\nName=Shared\nIsRelative=0\nPath={}\n",
            firefox.join("main").display()
        ),
    )
    .unwrap();
    let result = inventory(home.path(), &search(home.path()));
    assert_eq!(result.targets.len(), 1);
    assert_eq!(result.targets[0].database, firefox.join("work"));
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("associação ambígua"))
    );
    assert!(
        result.firefox_target().is_err(),
        "Conflito do perfil padrão não deve selecionar silenciosamente um secundário"
    );
}

#[test]
fn every_chromium_catalog_alias_is_discovered_outside_system_bin_without_creating_database() {
    for browser in super::super::chromium::BROWSERS {
        for command in browser.commands {
            let home = fixture();
            test_browser(&home.path().join("bin").join(command));
            let result = inventory(home.path(), &search(home.path()));
            assert_eq!(result.installed, [browser.name]);
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].family, Family::Chromium);
            assert_eq!(
                result.targets[0].installations[0].executables,
                [home.path().join("bin").join(command)]
            );
            assert!(!home.path().join(".pki").exists());
        }
    }
}

#[test]
fn hidden_user_desktop_shadows_system_entry_and_unavailable_tryexec_is_respected() {
    let home = fixture();
    let binary = home.path().join("opt/librewolf");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    test_browser(&binary);
    profile(home.path(), "config/librewolf/librewolf");
    fs::write(
        home.path().join("system-applications/librewolf.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nExec={} %u\n",
            binary.display()
        ),
    )
    .unwrap();
    fs::write(
        home.path().join("data/applications/librewolf.desktop"),
        "[Desktop Entry]\nType=Application\nHidden=true\n",
    )
    .unwrap();
    assert!(
        inventory(home.path(), &search(home.path()))
            .targets
            .is_empty()
    );
    fs::write(
        home.path().join("data/applications/librewolf.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nTryExec=/missing/librewolf\nExec={} %u\n",
            binary.display()
        ),
    )
    .unwrap();
    assert!(
        inventory(home.path(), &search(home.path()))
            .targets
            .is_empty()
    );
}

#[test]
fn multiple_native_locations_share_one_product_inventory() {
    let home = fixture();
    let extra = home.path().join("other-bin");
    fs::create_dir(&extra).unwrap();
    test_browser(&extra.join("librewolf"));
    test_browser(&home.path().join("bin/librewolf"));
    profile(home.path(), "config/librewolf/librewolf");
    let mut search = search(home.path());
    search.bins.push(extra);
    let result = inventory(home.path(), &search);
    assert_eq!(result.installed, ["LibreWolf"]);
    assert_eq!(result.targets.len(), 2);
    assert_eq!(result.targets[0].installations[0].executables.len(), 2);
}

#[test]
fn chromium_alias_cannot_bypass_process_protection_with_an_unknown_canonical_name() {
    let home = fixture();
    let binary = home.path().join("renamed-browser");
    test_browser(&binary);
    symlink(binary, home.path().join("bin/google-chrome")).unwrap();
    let result = inventory(home.path(), &search(home.path()));
    assert!(result.targets.is_empty());
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains("proteção Chromium"))
    );
}

#[test]
fn path_search_ignores_relative_entries_and_keeps_order_without_duplicates() {
    assert_eq!(
        absolute_unique(vec![
            PathBuf::from("."),
            PathBuf::from(""),
            PathBuf::from("/one"),
            PathBuf::from("relative"),
            PathBuf::from("/two"),
            PathBuf::from("/one")
        ]),
        [PathBuf::from("/one"), PathBuf::from("/two")]
    );
}
