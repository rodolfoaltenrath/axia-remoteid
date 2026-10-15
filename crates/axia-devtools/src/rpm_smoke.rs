use crate::{lab, nss, publication};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

pub fn smoke(root: &Path, preserved: Option<&Path>) -> Result<()> {
    lab::require_container()?;
    if let Some(home) = preserved {
        let snapshot: BTreeMap<PathBuf, String> =
            serde_json::from_slice(&fs::read(home.join("rpm-lab-snapshot.json"))?)?;
        for (path, digest) in snapshot {
            ensure!(
                publication::digest(&fs::read(path)?) == digest,
                "Remoção RPM alterou estado sintético"
            );
        }
        ensure!(
            !Path::new(desktopid_client::installation::SYSTEM_HELPER).exists()
                && !Path::new(nss::SYSTEM_MODULE).exists(),
            "Remoção RPM deixou runtime"
        );
        println!(
            "RPM removal verified: runtime removed; synthetic identity, wallet, preferences and startup entry preserved."
        );
        return Ok(());
    }
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Use usuário normal do contêiner"
    );
    let module = Path::new(nss::SYSTEM_MODULE);
    let helper = Path::new(desktopid_client::installation::SYSTEM_HELPER);
    ensure!(
        desktopid_client::installation::protected_system_file(module),
        "Biblioteca RPM desprotegida"
    );
    let launcher = Path::new("/usr/bin/axia-remotedid");
    let home = PathBuf::from(env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME ausente"))?);
    lab::successful(lab::command(launcher, &home).arg("--version"))?;
    ensure!(
        fs::read(launcher)?.starts_with(b"\x7fELF")
            && fs::metadata(launcher)?.ino() == fs::metadata(helper)?.ino(),
        "Entrada RPM não é ELF/hardlink"
    );
    let files = lab::successful(Command::new("rpm").args(["-ql", "axia-remotedid"]))?;
    ensure!(
        !String::from_utf8_lossy(&files.stdout)
            .lines()
            .any(|p| p.ends_with(".py") || p.ends_with(".sh") || p.contains("axia-devtools")),
        "RPM inclui scripts ou ferramentas de desenvolvimento"
    );
    let requirements =
        lab::successful(Command::new("rpm").args(["-q", "--requires", "axia-remotedid"]))?;
    ensure!(
        !["python3", "zenity", "firefox", "chromium"]
            .iter()
            .any(|s| String::from_utf8_lossy(&requirements.stdout).contains(s)),
        "RPM requer interpretador ou navegador diretamente"
    );
    let state = home.join(".config/desktopid-linux");
    ensure!(
        !state.exists(),
        "Teste RPM exige usuário com estado sintético novo"
    );
    lab::initialize(helper, &home, &state, &lab::fixture(root)?)?;
    let profile_root = home.join(".config/mozilla/firefox");
    let profile = profile_root.join("laboratory");
    fs::create_dir_all(&profile)?;
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))?;
    lab::private_write(
        &profile_root.join("profiles.ini"),
        b"[Profile0]\nPath=laboratory\nName=Laboratory\nDefault=1\n",
    )?;
    nss::Database::open(&profile)?.close()?;
    let legacy = home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so");
    fs::create_dir_all(legacy.parent().unwrap())?;
    fs::copy(module, &legacy)?;
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o700))?;
    let register = |module: &Path| -> Result<Value> {
        let result = lab::successful(
            lab::command(launcher, &home)
                .env("DESKTOPID_STATE_DIR", &state)
                .args(["setup-firefox", "--yes", "--profile"])
                .arg(&profile)
                .arg("--module")
                .arg(module),
        )?;
        Ok(serde_json::from_slice(&result.stdout)?)
    };
    register(&legacy)?;
    let mut snapshot = lab::snapshot(&state)?;
    let migrated = register(module)?;
    ensure!(
        migrated["migrated_legacy_module"] == true
            && migrated["certificate_count"] == 1
            && migrated["backup_directory"].is_string(),
        "Migração RPM não preservou backup/certificado"
    );
    ensure!(
        nss::registered_library(&profile)?.as_deref() == Some(module),
        "Módulo legado permaneceu registrado"
    );
    let again = register(module)?;
    ensure!(
        again["registered_now"] == false
            && again["migrated_legacy_module"] == false
            && again["backup_directory"].is_null(),
        "Migração RPM não idempotente"
    );
    let detected =
        lab::successful(lab::command(launcher, &home).args(["setup-browsers", "--list"]))?;
    let detected: Value = serde_json::from_slice(&detected.stdout)?;
    ensure!(
        detected["installed"].is_array() && detected["targets"].is_array(),
        "Inventário de navegadores inválido"
    );
    if detected["installed"]
        .as_array()
        .unwrap()
        .contains(&json!("Firefox"))
    {
        ensure!(
            detected["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["family"] == "firefox"),
            "Firefox instalado não detectou perfil"
        );
    }
    lab::successful(lab::command(launcher, &home).args(["autostart", "--enable"]))?;
    let entry = home.join(".config/autostart/axia-remotedid.desktop");
    ensure!(
        fs::read_to_string(&entry)?.contains(helper.to_str().unwrap()),
        "Autostart não selecionou auxiliar RPM protegido"
    );
    for (path, digest) in &snapshot {
        ensure!(
            publication::digest(&fs::read(path)?) == *digest,
            "Migração RPM alterou identidade/carteira"
        );
    }
    for path in [entry, home.join(".config/axia-remotedid/preferences.json")] {
        snapshot.insert(path.clone(), publication::digest(&fs::read(path)?));
    }
    lab::report(
        &home.join("rpm-lab-snapshot.json"),
        &serde_json::to_value(snapshot)?,
    )?;
    println!(
        "System RPM runtime: native ELF entry, no scripts/interpreter/browser requirement, protected library, Firefox migration, backup, idempotence, browser detection and autostart passed; no cloud requests."
    );
    Ok(())
}
