//! Native user installation and migration of recognized application shortcuts.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(crate) fn nonce() -> Result<String> {
    let mut bytes = [0; 16];
    openssl::rand::rand_bytes(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn owned_file(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },
        "Arquivo deve pertencer ao usuário e não ser um link"
    );
    Ok(metadata)
}

pub(crate) fn directory(path: &Path, home: &Path, private: bool) -> Result<()> {
    ensure!(
        path.is_absolute()
            && path.starts_with(home)
            && !path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Diretório deve ficar dentro do perfil Linux"
    );
    let existing = path
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .context("Diretório pai ausente")?;
    ensure!(
        existing.canonicalize()?.starts_with(home),
        "Diretório pai aponta para fora do perfil Linux"
    );
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0
            && path.canonicalize()?.starts_with(home),
        "Diretório deve pertencer ao usuário e ficar dentro do seu perfil Linux"
    );
    if private {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn replace(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if path.try_exists()? || path.is_symlink() {
        owned_file(path)?;
    }
    let temporary = path.with_extension(format!("{}.pending", nonce()?));
    let result = (|| {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// A small non-interpolating INI reader for browser and XDG metadata. Values
/// retain '=' and percent signs. Duplicate sections/keys are rejected.
pub(crate) fn ini(text: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut sections = BTreeMap::new();
    let mut current = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].to_owned();
            ensure!(
                !sections.contains_key(&name),
                "Seção repetida na configuração"
            );
            sections.insert(name.clone(), BTreeMap::new());
            current = Some(name);
        } else {
            let (key, value) = line
                .split_once('=')
                .context("Configuração de navegador inválida")?;
            let section = sections
                .get_mut(current.as_ref().context("Seção da configuração ausente")?)
                .context("Seção ausente")?;
            ensure!(
                section
                    .insert(key.trim().to_ascii_lowercase(), value.trim().to_owned())
                    .is_none(),
                "Chave repetida na configuração"
            );
        }
    }
    Ok(sections)
}

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME Linux não definido")?)
        .canonicalize()?;
    ensure!(home.is_absolute(), "HOME deve ser absoluto");
    Ok(home)
}

pub fn user() -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Abra o Axia RemotedID como seu usuário normal, sem sudo"
    );
    Ok(())
}

fn launcher(home: &Path) -> Result<PathBuf> {
    let directory = home.join(".local/share/applications");
    self::directory(&directory, home, false)?;
    let binary = home.join(".local/libexec/desktopid-linux/desktopid-client");
    let entry = directory.join("axia-remotedid-alpha.desktop");
    let argument = crate::autostart::argument(binary.to_str().context("Caminho inválido")?)?;
    let contents = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Axia RemotedID Alpha\nComment=Conectar certificados em nuvem Certisign\nExec={argument} gui\nTryExec={}\nIcon=dialog-password\nTerminal=false\nCategories=System;Security;\nActions=SetupBrowsers;\n\n[Desktop Action SetupBrowsers]\nName=Configurar navegadores\nExec={argument} setup-browsers\n",
        binary.display()
    );
    replace(&entry, contents.as_bytes(), 0o644)?;
    let commands = home.join(".local/bin");
    self::directory(&commands, home, false)?;
    let command = commands.join("axia-remotedid");
    if command.try_exists()? || command.is_symlink() {
        ensure!(
            command.is_symlink()
                && fs::symlink_metadata(&command)?.uid() == unsafe { libc::geteuid() }
                && command.canonicalize()? == binary.canonicalize()?,
            "Comando de outro aplicativo foi preservado"
        );
    } else {
        std::os::unix::fs::symlink(binary, command)?;
    }
    Ok(entry)
}

/// Install only compiled ELF artifacts; never read or replace account state.
pub fn install(target: &Path, home: &Path) -> Result<PathBuf> {
    let home = home.canonicalize()?;
    ensure!(
        !home.starts_with("/mnt"),
        "Instale no sistema de arquivos Linux"
    );
    for (source, destination) in [
        (
            target.join("desktopid-client"),
            home.join(".local/libexec/desktopid-linux/desktopid-client"),
        ),
        (
            target.join("libdesktopid_pkcs11.so"),
            home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so"),
        ),
    ] {
        let bytes = fs::read(source)?;
        ensure!(
            bytes.starts_with(b"\x7fELF"),
            "A instalação exige arquivos ELF compilados"
        );
        directory(
            destination.parent().context("Diretório ausente")?,
            &home,
            true,
        )?;
        replace(&destination, &bytes, 0o700)?;
    }
    launcher(&home)
}

/// Hide only our exact legacy entries after a protected system install exists.
pub fn migrate_shortcuts(home: &Path) -> Result<()> {
    let system = Path::new("/usr/bin/axia-remotedid");
    if !crate::installation::protected_system_file(system) {
        return Ok(());
    }
    let legacy = home.join(".local/libexec/desktopid-linux/desktopid-client");
    let argument = crate::autostart::argument(legacy.to_str().context("Caminho inválido")?)?;
    for name in [
        "desktopid-linux-alpha.desktop",
        "axia-remotedid-alpha.desktop",
    ] {
        let path = home.join(".local/share/applications").join(name);
        if !path.exists() || owned_file(&path).is_err() || !path.canonicalize()?.starts_with(home) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let sections = ini(&text)?;
        let Some(entry) = sections.get("Desktop Entry") else {
            continue;
        };
        if !matches!(
            entry.get("name").map(String::as_str),
            Some("DesktopID Linux Alpha" | "Axia RemotedID Alpha")
        ) || entry.get("exec") != Some(&format!("{argument} gui"))
            || entry.get("hidden").is_some_and(|value| value == "true")
        {
            continue;
        }
        let mut hidden = String::new();
        let mut in_entry = false;
        for line in text.lines() {
            if line.starts_with('[') {
                if in_entry {
                    hidden.push_str("Hidden=true\n");
                }
                in_entry = line == "[Desktop Entry]";
            }
            if !(in_entry && line.to_ascii_lowercase().starts_with("hidden=")) {
                hidden.push_str(line);
                hidden.push('\n');
            }
        }
        if in_entry {
            hidden.push_str("Hidden=true\n");
        }
        replace(&path, hidden.as_bytes(), 0o644)?;
    }
    let command = home.join(".local/bin/axia-remotedid");
    if command.is_symlink()
        && fs::symlink_metadata(&command)?.uid() == unsafe { libc::geteuid() }
        && command
            .canonicalize()
            .ok()
            .zip(legacy.canonicalize().ok())
            .is_some_and(|(command, legacy)| command == legacy)
        && command
            .parent()
            .context("Diretório ausente")?
            .canonicalize()?
            .starts_with(home)
    {
        let temporary = command.with_extension(format!("{}.pending", nonce()?));
        std::os::unix::fs::symlink(system, &temporary)?;
        fs::rename(temporary, command)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_install_preserves_account_and_refuses_foreign_launcher() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().join("home space $value %f");
        fs::create_dir(&home).unwrap();
        let target = temporary.path().join("build");
        fs::create_dir(&target).unwrap();
        for name in ["desktopid-client", "libdesktopid_pkcs11.so"] {
            fs::write(target.join(name), b"\x7fELFlaboratory").unwrap();
        }
        let identity = home.join(".config/desktopid-linux/sentinel");
        fs::create_dir_all(identity.parent().unwrap()).unwrap();
        fs::write(&identity, b"preserve").unwrap();
        let entry = install(&target, &home).unwrap();
        assert_eq!(fs::read(identity).unwrap(), b"preserve");
        assert_eq!(
            fs::metadata(home.join(".local/libexec/desktopid-linux/desktopid-client"))
                .unwrap()
                .mode()
                & 0o777,
            0o700
        );
        let text = fs::read_to_string(&entry).unwrap();
        assert!(text.contains("%%f") && text.contains("Actions=SetupBrowsers"));
        assert!(
            std::process::Command::new("desktop-file-validate")
                .arg(&entry)
                .status()
                .unwrap()
                .success()
        );
        fs::remove_file(&entry).unwrap();
        let other = home.join("unrelated");
        fs::write(&other, b"preserve").unwrap();
        std::os::unix::fs::symlink(&other, entry).unwrap();
        assert!(install(&target, &home).is_err());
        assert_eq!(fs::read(other).unwrap(), b"preserve");
    }
}
