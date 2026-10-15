use super::*;

#[test]
fn native_install_repairs_only_our_obsolete_system_alias_and_preserves_account() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let target = home.join("build");
    fs::create_dir(&target).unwrap();
    for name in ["desktopid-client", "libdesktopid_pkcs11.so"] {
        fs::write(
            target.join(name),
            [
                b"\x7fELFlaboratory".as_slice(),
                crate::installation::runtime::MARKER,
            ]
            .concat(),
        )
        .unwrap();
    }
    let alias = home.join(".local/bin/axia-remotedid");
    fs::create_dir_all(alias.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(crate::installation::SYSTEM_LAUNCHER, &alias).unwrap();
    let state = home.join(".config/desktopid-linux/sentinel");
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(&state, b"preserve laboratory account").unwrap();
    // No native RPM is installed by this unit test. The alias may be dangling.
    if crate::installation::system_helper().is_none() {
        install(&target, home).unwrap();
        assert_eq!(
            fs::read_link(&alias).unwrap(),
            home.join(".local/libexec/desktopid-linux/desktopid-client")
        );
        assert_eq!(fs::read(state).unwrap(), b"preserve laboratory account");
    }
    fs::remove_file(&alias).unwrap();
    let foreign = home.join("other-application");
    fs::write(&foreign, b"preserve").unwrap();
    std::os::unix::fs::symlink(&foreign, &alias).unwrap();
    assert!(install(&target, home).is_err());
    assert_eq!(fs::read_link(alias).unwrap(), foreign);
    assert_eq!(fs::read(foreign).unwrap(), b"preserve");
}

#[test]
fn mixed_installation_is_rejected_before_replacing_existing_runtime_files() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let target = home.join("build");
    fs::create_dir(&target).unwrap();
    for name in ["desktopid-client", "libdesktopid_pkcs11.so"] {
        fs::write(
            target.join(name),
            [
                b"\x7fELFlaboratory".as_slice(),
                crate::installation::runtime::MARKER,
            ]
            .concat(),
        )
        .unwrap();
    }
    install(&target, home).unwrap();
    let helper = home.join(".local/libexec/desktopid-linux/desktopid-client");
    let module = home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so");
    let before_helper = fs::read(&helper).unwrap();
    let before_module = fs::read(&module).unwrap();
    fs::write(
        target.join("desktopid-client"),
        [
            b"\x7fELFnew native helper".as_slice(),
            crate::installation::runtime::MARKER,
        ]
        .concat(),
    )
    .unwrap();
    fs::copy("/usr/bin/true", target.join("libdesktopid_pkcs11.so")).unwrap();
    assert!(install(&target, home).is_err());
    assert_eq!(fs::read(helper).unwrap(), before_helper);
    assert_eq!(fs::read(module).unwrap(), before_module);
}
