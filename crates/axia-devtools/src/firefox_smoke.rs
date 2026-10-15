use crate::{lab, lab_helper, nss};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{env, fs, os::unix::fs::PermissionsExt, path::Path};

pub fn smoke(root: &Path, module: &Path, client: &Path) -> Result<()> {
    let module = module.canonicalize()?;
    let client = client.canonicalize()?;
    let temp = tempfile::Builder::new()
        .prefix("axia-firefox-lab-")
        .tempdir()?;
    let home = temp.path();
    lab_helper::mark(home)?;
    lab::initialize(&client, home, &home.join("identity"), &lab::fixture(root)?)?;
    let installed = home.join("libdesktopid_pkcs11.so");
    fs::copy(module, &installed)?;
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o700))?;
    let result = lab::successful(
        lab::command(&env::current_exe()?, home)
            .arg("firefox-worker")
            .arg("--module")
            .arg(installed)
            .arg("--client")
            .arg(client)
            .arg("--home")
            .arg(home),
    )?;
    print!("{}", String::from_utf8_lossy(&result.stdout));
    Ok(())
}

pub fn worker(module: &Path, client: &Path, home: &Path) -> Result<()> {
    lab_helper::validate_home(home)?;
    let state = home.join("identity");
    let profile = home.join("firefox");
    fs::create_dir(&profile)?;
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))?;
    nss::Database::open(&profile)?.close()?;
    let before: Vec<_> = nss::FILES
        .iter()
        .map(|name| Ok((*name, fs::read(profile.join(name))?)))
        .collect::<Result<_>>()?;
    let register = |path: &Path, success: bool| -> Result<Value> {
        let result = lab::capture(
            lab::command(client, home)
                .arg("--state-dir")
                .arg(&state)
                .args(["setup-firefox", "--yes", "--profile"])
                .arg(&profile)
                .arg("--home")
                .arg(home)
                .arg("--module")
                .arg(path),
            std::time::Duration::from_secs(30),
            b"",
        )?;
        ensure!(
            result.status.success() == success,
            "Registro NSS: resultado inesperado: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if success {
            Ok(serde_json::from_slice(&result.stdout)?)
        } else {
            Ok(Value::String(
                String::from_utf8_lossy(&result.stderr).into_owned(),
            ))
        }
    };
    let invalid = home.join("invalid-provider.so");
    lab::private_write(&invalid, b"Laboratory invalid shared object")?;
    fs::set_permissions(&invalid, fs::Permissions::from_mode(0o700))?;
    register(&invalid, false)?;
    for (name, bytes) in &before {
        ensure!(
            fs::read(profile.join(name))? == *bytes,
            "Falha NSS não restaurou base"
        );
    }
    let first = register(module, true)?;
    ensure!(
        first["registered_now"] == true
            && first["module_loaded_by_nss"] == true
            && first["certificate_count"] == 1,
        "Módulo/certificado NSS não carregou"
    );
    let backup = Path::new(first["backup_directory"].as_str().unwrap());
    ensure!(lab::mode(backup)? == 0o700, "Backup NSS não privado");
    for (name, bytes) in &before {
        ensure!(
            fs::read(backup.join(name))? == *bytes && lab::mode(&backup.join(name))? == 0o600,
            "Backup NSS divergiu ou não é privado"
        );
    }
    let saved = fs::read(profile.join("pkcs11.txt"))?;
    let second = register(module, true)?;
    ensure!(
        second["registered_now"] == false
            && second["backup_directory"].is_null()
            && second["certificate_count"] == 1
            && fs::read(profile.join("pkcs11.txt"))? == saved,
        "Registro NSS não idempotente"
    );
    let database = nss::Database::open(&profile)?;
    database.check_key_without_login()?;
    database.close()?;
    let conflicting = home.join("different-module.so");
    fs::copy(module, &conflicting)?;
    fs::set_permissions(&conflicting, fs::Permissions::from_mode(0o700))?;
    let error = register(&conflicting, false)?;
    ensure!(
        error.as_str().unwrap().contains("preservada")
            && fs::read(profile.join("pkcs11.txt"))? == saved,
        "Conflito NSS alterou registro"
    );
    ensure!(
        nss::registered_library(&profile)?.as_deref() == Some(module),
        "Registro NSS mudou biblioteca"
    );
    println!(
        "Firefox/NSS: load, certificate/key discovery without password callback, persistence, idempotence, private backup, rollback and conflict checks passed; no cloud calls."
    );
    Ok(())
}
