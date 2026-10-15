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
    // Reproduce an obsolete or removed alpha.1 RPM registration without
    // installing anything in /usr or touching a real browser profile.
    if desktopid_client::installation::runtime::system_module().is_none() {
        let user_module = home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so");
        let user_helper = home.join(".local/libexec/desktopid-linux/desktopid-client");
        for path in [&user_module, &user_helper] {
            fs::create_dir_all(path.parent().unwrap())?;
        }
        fs::copy(client, &user_helper)?;
        fs::set_permissions(&user_helper, fs::Permissions::from_mode(0o700))?;
        let old_registration =
            String::from_utf8(saved)?.replace(module.to_str().unwrap(), nss::SYSTEM_MODULE);
        lab::private_write(&profile.join("pkcs11.txt"), old_registration.as_bytes())?;
        ensure!(
            nss::registered_library(&profile)?.as_deref() == Some(Path::new(nss::SYSTEM_MODULE)),
            "Fixture NSS não apontou para o RPM anterior"
        );
        // This looks compatible to the selection preflight but is deliberately
        // not a loadable library, so NSS must restore the previous registration.
        lab::private_write(
            &user_module,
            &[
                b"\x7fELFinvalid laboratory provider".as_slice(),
                desktopid_client::installation::runtime::MARKER,
            ]
            .concat(),
        )?;
        fs::set_permissions(&user_module, fs::Permissions::from_mode(0o700))?;
        register(&user_module, false)?;
        ensure!(
            fs::read(profile.join("pkcs11.txt"))? == old_registration.as_bytes(),
            "Falha da migração RPM antigo/usuário não restaurou o registro"
        );
        fs::copy(module, &user_module)?;
        fs::set_permissions(&user_module, fs::Permissions::from_mode(0o700))?;
        let migrated = register(&user_module, true)?;
        ensure!(
            migrated["migrated_legacy_module"] == true
                && migrated["certificate_count"] == 1
                && migrated["backup_directory"].is_string(),
            "Migração RPM antigo/usuário não concluída com backup"
        );
        let backup = Path::new(migrated["backup_directory"].as_str().unwrap());
        ensure!(
            fs::read(backup.join("pkcs11.txt"))? == old_registration.as_bytes()
                && lab::mode(backup)? == 0o700,
            "Backup do módulo antigo não foi preservado privadamente"
        );
        let repeated = register(&user_module, true)?;
        ensure!(
            repeated["registered_now"] == false
                && repeated["migrated_legacy_module"] == false
                && repeated["backup_directory"].is_null(),
            "Migração repetida não é idempotente"
        );
        println!(
            "Firefox/NSS: obsolete RPM to native user runtime, failed migration rollback, private backup and idempotence passed."
        );
    }
    println!(
        "Firefox/NSS: load, certificate/key discovery without password callback, persistence, idempotence, private backup, rollback and conflict checks passed; no cloud calls."
    );
    Ok(())
}
