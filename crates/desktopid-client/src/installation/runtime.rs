//! Select a compatible native module/helper without loading or executing either.
use super::{protected_system_file, system_helper};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const SYSTEM_MODULE: &str = "/usr/lib64/axia-remotedid/libdesktopid_pkcs11.so";
const USER_MODULE: &str = ".local/lib/desktopid-linux/libdesktopid_pkcs11.so";
const USER_HELPER: &str = ".local/libexec/desktopid-linux/desktopid-client";
pub const MARKER: &[u8] = b"AXIA_REMOTEID_NATIVE_RUNTIME_V2\0";

/// A compatibility identifier, not a signature or proof of binary provenance.
/// Keep it reachable in both final ELF artifacts, including stripped releases.
pub fn retain_marker() {
    std::hint::black_box(MARKER);
}

pub fn native_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF") && bytes.windows(MARKER.len()).any(|window| window == MARKER)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Runtime {
    pub module: PathBuf,
    pub helper: PathBuf,
}

/// Existing alpha.1 ELF files have no native-runtime identifier. Checking only
/// ELF magic would still accept their interpreter-based authorization helper.
pub(super) fn native_artifact(path: &Path) -> bool {
    let Ok(mut file) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut magic = [0; 4];
    if file.read_exact(&mut magic).is_err() || magic != *b"\x7fELF" {
        return false;
    }
    let mut buffer = vec![0; 65536 + MARKER.len()];
    let mut retained = 0;
    loop {
        let Ok(count) = file.read(&mut buffer[retained..]) else {
            return false;
        };
        if count == 0 {
            return false;
        }
        let size = retained + count;
        if buffer[..size]
            .windows(MARKER.len())
            .any(|window| window == MARKER)
        {
            return true;
        }
        retained = (MARKER.len() - 1).min(size);
        buffer.copy_within(size - retained..size, 0);
    }
}

pub(super) fn protected_module(path: &Path) -> bool {
    protected_system_file(path) && native_artifact(path)
}

pub fn system_module() -> Option<&'static Path> {
    let module = Path::new(SYSTEM_MODULE);
    (system_helper().is_some() && protected_module(module)).then_some(module)
}

fn user_file(path: &Path, home: &Path, executable: bool) -> Result<()> {
    ensure!(
        path.starts_with(home),
        "Runtime deve ficar no perfil Linux do usuário"
    );
    for ancestor in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(ancestor)?;
        ensure!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o022 == 0,
            "Diretório do runtime deve pertencer ao usuário e não permitir escrita por terceiros"
        );
        if ancestor == home {
            break;
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0
            && (!executable || metadata.mode() & 0o100 != 0),
        "Arquivo do runtime possui tipo, proprietário ou permissões inválidas"
    );
    ensure!(
        native_artifact(path),
        "Runtime antigo ou incompatível; reinstale o módulo e o auxiliar da versão nativa atual"
    );
    Ok(())
}

pub fn user_runtime(home: &Path) -> Result<Runtime> {
    let home = home.canonicalize()?;
    let runtime = Runtime {
        module: home.join(USER_MODULE),
        helper: home.join(USER_HELPER),
    };
    user_file(&runtime.module, &home, false)
        .context("Instale o Axia RemotedID nativo para este usuário")?;
    user_file(&runtime.helper, &home, true)
        .context("Instale o módulo e o auxiliar nativos juntos para este usuário")?;
    Ok(runtime)
}

pub fn select(home: &Path) -> Result<Runtime> {
    select_with(
        home,
        Path::new(SYSTEM_MODULE),
        system_helper(),
        protected_module,
    )
}

fn select_with(
    home: &Path,
    module: &Path,
    helper: Option<&Path>,
    protected: impl FnOnce(&Path) -> bool,
) -> Result<Runtime> {
    if let Some(helper) = helper
        && protected(module)
    {
        return Ok(Runtime {
            module: module.to_owned(),
            helper: helper.to_owned(),
        });
    }
    user_runtime(home)
}

/// Only the exact obsolete system registration can move back to our current
/// private installation. Caller still requires explicit consent and NSS backup.
pub fn can_replace_old_system(home: &Path, existing: &Path, desired: &Path) -> bool {
    can_replace_with(
        home,
        existing,
        desired,
        system_module(),
        |path| match fs::symlink_metadata(path) {
            Ok(_) => protected_system_file(path),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        },
    )
}

fn can_replace_with(
    home: &Path,
    existing: &Path,
    desired: &Path,
    current_system: Option<&Path>,
    old_system_safe: impl FnOnce(&Path) -> bool,
) -> bool {
    existing == Path::new(SYSTEM_MODULE)
        && desired == home.join(USER_MODULE)
        && current_system.is_none()
        && old_system_safe(existing)
        && user_runtime(home).is_ok_and(|runtime| runtime.module == desired)
}

#[cfg(test)]
mod tests;
