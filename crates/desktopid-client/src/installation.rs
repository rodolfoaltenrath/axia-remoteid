//! Trusted system runtime installed by the Fedora RPM.
use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub const SYSTEM_HELPER: &str = "/usr/libexec/axia-remotedid/desktopid-client";

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
    protected_system_file(path).then_some(path)
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
}
