//! Trusted system runtime installed by the Fedora RPM.
use std::{fs, io::Read, os::unix::fs::MetadataExt, path::Path};

pub const SYSTEM_HELPER: &str = "/usr/libexec/axia-remotedid/desktopid-client";
pub const SYSTEM_LAUNCHER: &str = "/usr/bin/axia-remotedid";

pub fn protected_system_file(path: &Path) -> bool {
    path.is_absolute()
        && path.ancestors().enumerate().all(|(index, ancestor)| {
            fs::symlink_metadata(ancestor).is_ok_and(|metadata| {
                metadata.uid() == 0
                    && metadata.mode() & 0o022 == 0
                    && if index == 0 {
                        metadata.is_file() && metadata.mode() & 0o111 != 0
                    } else {
                        metadata.is_dir()
                    }
            })
        })
}

pub fn system_helper() -> Option<&'static Path> {
    let path = Path::new(SYSTEM_HELPER);
    let launcher = Path::new(SYSTEM_LAUNCHER);
    (protected_system_file(path)
        && protected_system_file(launcher)
        && same_native_executable(path, launcher))
    .then_some(path)
}

// The alpha.2 RPM installs these paths as hardlinks to one ELF. The alpha.1
// launcher is a shell wrapper that calls Python; never select that old runtime
// over a newly installed native frontend in the user's home.
fn same_native_executable(helper: &Path, launcher: &Path) -> bool {
    let Ok(helper_metadata) = fs::symlink_metadata(helper) else {
        return false;
    };
    let Ok(launcher_metadata) = fs::symlink_metadata(launcher) else {
        return false;
    };
    if !helper_metadata.is_file()
        || !launcher_metadata.is_file()
        || helper_metadata.dev() != launcher_metadata.dev()
        || helper_metadata.ino() != launcher_metadata.ino()
    {
        return false;
    }
    let mut header = [0; 4];
    fs::File::open(helper)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && header == *b"\x7fELF"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_runtime_requires_root_owned_protected_ancestors_and_no_symlinks() {
        assert!(protected_system_file(Path::new("/usr/bin/true")));
        assert!(!protected_system_file(Path::new("usr/bin/true")));
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("helper");
        fs::write(&file, b"laboratory").unwrap();
        // /tmp is writable by others, including when tests run as root.
        assert!(!protected_system_file(&file));
        let link = temporary.path().join("link");
        std::os::unix::fs::symlink("/usr/bin/true", &link).unwrap();
        assert!(!protected_system_file(&link));
    }

    #[test]
    fn native_runtime_rejects_legacy_wrapper_scripts_and_requires_one_elf_inode() {
        let temporary = tempfile::tempdir().unwrap();
        let helper = temporary.path().join("helper");
        let launcher = temporary.path().join("launcher");
        fs::copy("/usr/bin/true", &helper).unwrap();
        fs::write(&launcher, b"#!/bin/sh\nexec python3 legacy.py\n").unwrap();
        assert!(!same_native_executable(&helper, &launcher));
        fs::remove_file(&launcher).unwrap();
        fs::copy(&helper, &launcher).unwrap();
        assert!(!same_native_executable(&helper, &launcher));
        fs::remove_file(&launcher).unwrap();
        fs::hard_link(&helper, &launcher).unwrap();
        assert!(same_native_executable(&helper, &launcher));
        fs::write(&helper, b"#!/bin/sh\nexit 0\n").unwrap();
        assert!(!same_native_executable(&helper, &launcher));
        fs::remove_file(&launcher).unwrap();
        std::os::unix::fs::symlink(&helper, &launcher).unwrap();
        assert!(!same_native_executable(&helper, &launcher));
    }
}
