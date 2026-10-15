//! First-run preference and XDG desktop autostart, independent of account state.
use crate::storage::{check_owned_private, write_new_private};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PRODUCT_NAME: &str = "Axia RemotedID";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    schema_version: u32,
    autostart_enabled: bool,
}

pub fn config_root() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME Linux não definido")?)
            .join(".config"),
    };
    ensure!(root.is_absolute(), "XDG_CONFIG_HOME deve ser absoluto");
    Ok(root)
}

fn preference_path(root: &Path) -> PathBuf {
    root.join("axia-remotedid/preferences.json")
}
fn entry_path(root: &Path) -> PathBuf {
    root.join("autostart/axia-remotedid.desktop")
}

fn owned_directory(path: &Path, private: bool) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == unsafe { libc::geteuid() },
        "Diretório de preferências deve pertencer ao usuário e não ser um link"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "Diretório de preferências não deve ser gravável por outros usuários"
    );
    if private {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn replace_owned(path: &Path, data: &[u8]) -> Result<()> {
    if path.try_exists()? || path.is_symlink() {
        check_owned_private(path, false)?;
    }
    let mut nonce = [0u8; 16];
    openssl::rand::rand_bytes(&mut nonce)?;
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let temporary = path.with_extension(format!("{nonce}.pending"));
    write_new_private(&temporary, data)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn load(root: &Path) -> Result<Option<Preferences>> {
    let path = preference_path(root);
    if !path.try_exists()? && !path.is_symlink() {
        return Ok(None);
    }
    check_owned_private(
        path.parent().context("Diretório de preferências ausente")?,
        true,
    )?;
    check_owned_private(&path, false)?;
    let preferences: Preferences = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        preferences.schema_version == 1,
        "Versão de preferências incompatível"
    );
    Ok(Some(preferences))
}

fn argument(value: &str) -> Result<String> {
    ensure!(
        !value
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0')),
        "Caminho inválido para inicialização automática"
    );
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                escaped.push_str("\\\\");
                escaped.push(character);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    Ok(escaped)
}

pub fn executable() -> Result<PathBuf> {
    if let Some(system) = crate::installation::system_helper() {
        return Ok(system.to_owned());
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME Linux não definido")?);
    let installed = home.join(".local/libexec/desktopid-linux/desktopid-client");
    if installed.try_exists()? {
        let metadata = fs::symlink_metadata(&installed)?;
        ensure!(
            metadata.is_file()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o022 == 0,
            "Executável instalado não é um arquivo protegido do usuário"
        );
        Ok(installed)
    } else {
        Ok(std::env::current_exe()?)
    }
}

pub fn enabled(root: &Path) -> Result<bool> {
    Ok(load(root)?.is_some_and(|preferences| preferences.autostart_enabled))
}

pub fn configure(root: &Path, binary: &Path, state: &Path, enable: bool) -> Result<()> {
    ensure!(
        root.is_absolute() && binary.is_absolute() && state.is_absolute(),
        "Caminhos de inicialização devem ser absolutos"
    );
    let binary_text = binary.to_str().context("Caminho do executável inválido")?;
    ensure!(
        !binary_text.contains('='),
        "Caminho do executável incompatível com atalhos XDG"
    );
    let entry = entry_path(root);
    if enable || entry.try_exists()? || entry.is_symlink() {
        owned_directory(
            entry.parent().context("Diretório autostart ausente")?,
            false,
        )?;
        let command = format!(
            "{} --state-dir {} startup",
            argument(binary_text)?,
            argument(state.to_str().context("Caminho de estado inválido")?)?
        );
        let content = format!(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName={PRODUCT_NAME}\nComment=Iniciar o Axia RemotedID na bandeja, sem abrir janela\nExec={command}\nTryExec={binary_text}\nIcon=dialog-password\nTerminal=false\nHidden={}\n",
            !enable
        );
        replace_owned(&entry, content.as_bytes())?;
    }
    let preferences = preference_path(root);
    owned_directory(
        preferences
            .parent()
            .context("Diretório de preferências ausente")?,
        true,
    )?;
    replace_owned(
        &preferences,
        &serde_json::to_vec_pretty(&Preferences {
            schema_version: 1,
            autostart_enabled: enable,
        })?,
    )
}

pub(crate) fn first_run(
    root: &Path,
    binary: &Path,
    state: &Path,
    decide: impl FnOnce() -> Result<bool>,
) -> Result<()> {
    if load(root)?.is_none() {
        configure(root, binary, state, decide()?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_run_records_decline_once_without_enabling_or_creating_account_state() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("config");
        let state = temporary.path().join("identity");
        let binary = temporary.path().join("program");
        first_run(&root, &binary, &state, || Ok(false)).unwrap();
        first_run(&root, &binary, &state, || {
            panic!("The first-run question must not repeat")
        })
        .unwrap();
        assert!(!enabled(&root).unwrap());
        assert!(!entry_path(&root).exists());
        assert!(!state.exists());
    }
    #[test]
    fn enable_and_disable_preserve_state_and_quote_desktop_arguments() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("config");
        let state = temporary.path().join("state $value %f");
        fs::create_dir(&state).unwrap();
        fs::write(state.join("sentinel"), b"preserve").unwrap();
        let binary = temporary.path().join("axia remotedid");
        configure(&root, &binary, &state, true).unwrap();
        assert!(enabled(&root).unwrap());
        let entry = fs::read_to_string(entry_path(&root)).unwrap();
        assert!(entry.contains("Name=Axia RemotedID"));
        assert!(entry.contains("Hidden=false"));
        assert!(entry.contains("\\\\$value %%f"));
        if std::process::Command::new("desktop-file-validate")
            .arg(entry_path(&root))
            .output()
            .is_ok_and(|result| !result.status.success())
        {
            panic!("Invalid desktop autostart entry");
        }
        configure(&root, &binary, &state, false).unwrap();
        assert!(!enabled(&root).unwrap());
        assert!(
            fs::read_to_string(entry_path(&root))
                .unwrap()
                .contains("Hidden=true")
        );
        assert_eq!(fs::read(state.join("sentinel")).unwrap(), b"preserve");
        assert_eq!(
            fs::metadata(preference_path(&root)).unwrap().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn autostart_refuses_symlink_without_overwriting_other_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("config");
        fs::create_dir_all(root.join("autostart")).unwrap();
        let target = temporary.path().join("unrelated");
        fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, entry_path(&root)).unwrap();
        assert!(
            configure(
                &root,
                &temporary.path().join("program"),
                &temporary.path().join("state"),
                true
            )
            .is_err()
        );
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
