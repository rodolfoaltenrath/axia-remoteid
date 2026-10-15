//! Compiled synthetic broker; enabled only under a private laboratory home.
use crate::{lab, publication};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use desktopid_client::{protocol::Algorithm, storage::check_owned_private};
use openssl::{md::Md, pkey::PKey, pkey_ctx::PkeyCtx, rsa::Padding};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const NAME: &str = "axia-laboratory-helper";
pub fn install(home: &Path) -> Result<PathBuf> {
    let path = home.join(NAME);
    fs::copy(env::current_exe()?, &path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

pub fn validate_home(home: &Path) -> Result<()> {
    check_owned_private(home, true)?;
    ensure!(
        home.join(".axia-laboratory").is_file(),
        "Marcador de laboratório obrigatório"
    );
    ensure!(home.is_absolute(), "Home de laboratório deve ser absoluto");
    Ok(())
}

pub fn mark(home: &Path) -> Result<()> {
    fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    lab::private_write(&home.join(".axia-laboratory"), b"synthetic only\n")
}

pub fn broker() -> Result<()> {
    let home = PathBuf::from(
        env::var_os("AXIA_LAB_HOME").context("Broker disponível somente no laboratório")?,
    );
    validate_home(&home)?;
    ensure!(
        PathBuf::from(env::var_os("DESKTOPID_STATE_DIR").context("Estado sintético ausente")?)
            .starts_with(&home),
        "Estado fora do laboratório"
    );
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../desktopid-client/tests/fixtures/python-vectors.json"
    ))?;
    let key = PKey::private_key_from_pem(fixture["private_key_pem"].as_str().unwrap().as_bytes())?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        ensure!(line.len() <= 65536, "Pedido de laboratório excede limite");
        let request: Value = serde_json::from_str(&line)?;
        let mode = fs::read_to_string(home.join("helper-mode")).unwrap_or_default();
        if mode == "cancel" {
            std::process::exit(2);
        }
        let algorithm = request["algorithm"].as_str().context("Algoritmo ausente")?;
        let (md, size) = match algorithm {
            "SHA256" => (Md::sha256(), Algorithm::Sha256.digest_len()),
            "SHA384" => (Md::sha384(), Algorithm::Sha384.digest_len()),
            "SHA512" => (Md::sha512(), Algorithm::Sha512.digest_len()),
            _ => anyhow::bail!("Algoritmo fora do laboratório"),
        };
        let digest = lab::decode(&request["digest_base64"])?;
        ensure!(digest.len() == size, "Digest de laboratório incorreto");
        let mut signer = PkeyCtx::new(&key)?;
        signer.sign_init()?;
        signer.set_rsa_padding(Padding::PKCS1)?;
        signer.set_signature_md(md)?;
        let mut signature = Vec::new();
        signer.sign_to_vec(&digest, &mut signature)?;
        if mode == "invalid" {
            signature.fill(0);
        }
        let mut counter = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(home.join("calls.jsonl"))?;
        writeln!(
            counter,
            "{}",
            json!({"algorithm":algorithm,"helper_pid":std::process::id()})
        )?;
        writeln!(
            stdout,
            "{}",
            json!({"schema_version":1,"signature_base64":STANDARD.encode(signature)})
        )?;
        stdout.flush()?;
    }
    Ok(())
}

pub fn called_as_helper() -> bool {
    env::args_os()
        .next()
        .is_some_and(|arg| Path::new(&arg).file_name().is_some_and(|name| name == NAME))
}

pub fn inventory_worker(root: &Path, module: &Path, client: &Path) -> Result<()> {
    let temp = tempfile::Builder::new()
        .prefix("axia-inventory-lab-")
        .tempdir()?;
    let home = temp.path();
    mark(home)?;
    let state = home.join("identity");
    lab::initialize(client, home, &state, &lab::fixture(root)?)?;
    let original = lab::snapshot(&state)?;
    let result = lab::successful(
        lab::command(&env::current_exe()?, home)
            .args(["pkcs11-inventory", "--module"])
            .arg(module)
            .env("DESKTOPID_STATE_DIR", &state),
    )?;
    let value: Value = serde_json::from_slice(&result.stdout)?;
    for name in [
        "certificate_count",
        "public_key_count",
        "private_references_before_local_login",
        "private_key_reference_count",
    ] {
        ensure!(value[name] == 1, "Inventário sintético perdeu objetos");
    }
    ensure!(
        original == lab::snapshot(&state)? && !home.join("calls.jsonl").exists(),
        "Inventário alterou estado ou chamou auxiliar"
    );
    ensure!(
        value["remote_signature_requested"] == false && value["credentials_requested"] == false,
        "Inventário solicitou credenciais"
    );
    // All checks complete without loading the account private key.
    fs::remove_file(state.join("identity.pem"))?;
    let result = lab::successful(
        lab::command(&env::current_exe()?, home)
            .args(["pkcs11-inventory", "--module"])
            .arg(module)
            .env("DESKTOPID_STATE_DIR", &state),
    )?;
    ensure!(
        serde_json::from_slice::<Value>(&result.stdout)? == value,
        "Inventário dependeu da chave da instalação"
    );
    println!(
        "PKCS11 inventory: counts, local optional login, no private identity read, no credentials or signatures passed."
    );
    Ok(())
}

pub fn fingerprint(path: &Path) -> Result<String> {
    Ok(publication::digest(&fs::read(path)?))
}

pub fn interactive_test(module: &Path, home: &Path) -> Result<()> {
    use std::{os::fd::FromRawFd, process::Stdio, time::Duration};
    let state = home.join("identity");
    let before = lab::snapshot(&state)?;
    let executable = env::current_exe()?;
    let configure = || {
        let mut command = lab::command(&executable, home);
        command
            .args(["sign-pkcs11-test", "--certificate-index", "1", "--module"])
            .arg(module)
            .arg("--helper")
            .arg(home.join(NAME))
            .arg("--state-dir")
            .arg(&state)
            .env("AXIA_LAB_HOME", home);
        command
    };
    let failed = lab::capture(&mut configure(), Duration::from_secs(30), b"")?;
    ensure!(
        !failed.status.success()
            && String::from_utf8_lossy(&failed.stderr).contains("terminal interativo")
            && !state.join("pkcs11-tests").exists(),
        "Teste não interativo solicitou assinatura"
    );
    for (mode, success) in [("", true), ("cancel", false), ("invalid", false)] {
        let mut master = -1;
        let mut slave = -1;
        // SAFETY: valid fd pointers; default terminal attributes, no real TTY.
        ensure!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0,
            "Não foi possível criar PTY privado"
        );
        // SAFETY: openpty returned these two newly owned descriptors.
        let _master = unsafe { fs::File::from_raw_fd(master) };
        let slave = unsafe { fs::File::from_raw_fd(slave) };
        lab::private_write(&home.join("helper-mode"), mode.as_bytes())?;
        let mut command = configure();
        command
            .stdin(Stdio::from(slave))
            .env("DISPLAY", ":axia-synthetic-no-real-display");
        let result = lab::capture_terminal(&mut command, Duration::from_secs(30), b"")?;
        ensure!(
            result.status.success() == success,
            "Teste interativo sintético divergiu: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let directories =
        fs::read_dir(state.join("pkcs11-tests"))?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        directories.len() == 3,
        "Teste real repetiu tentativa automaticamente"
    );
    let mut results = 0;
    for entry in directories {
        let directory = entry.path();
        ensure!(
            lab::mode(&directory)? == 0o700 && lab::mode(&directory.join("attempt.json"))? == 0o600,
            "Artefatos de assinatura não privados"
        );
        if directory.join("result.json").exists() {
            results += 1;
            let value: Value = serde_json::from_slice(&fs::read(directory.join("result.json"))?)?;
            ensure!(
                value["signature_verified"] == true
                    && lab::mode(&directory.join("signature.bin"))? == 0o600,
                "Assinatura sintética não foi verificada"
            );
        }
    }
    ensure!(results == 1, "Cancelamento/assinatura inválida foi aceita");
    for (path, digest) in before {
        ensure!(
            fingerprint(&path)? == digest,
            "Teste interativo alterou identidade/carteira"
        );
    }
    println!(
        "Interactive OpenSC test: private PTY, noninteractive guard, verified synthetic signature, cancellation and invalid response without retry passed; no cloud calls."
    );
    Ok(())
}
