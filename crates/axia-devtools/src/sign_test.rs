//! Explicit interactive OpenSC test; no automatic invocation during validation.
use crate::{lab, publication};
use anyhow::{Context, Result, ensure};
use desktopid_client::{
    protocol::{Algorithm, verify_digest},
    storage::{Store, check_owned_private, write_new_private},
};
use openssl::hash::{MessageDigest, hash};
use serde_json::json;
use std::{
    env, fs,
    io::{IsTerminal, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub fn paths(
    module: Option<PathBuf>,
    helper: Option<PathBuf>,
    state: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let home = PathBuf::from(env::var_os("HOME").context("HOME ausente")?);
    let runtime = if module.is_none() || helper.is_none() {
        Some(desktopid_client::installation::runtime::select(&home)?)
    } else {
        None
    };
    let module = module
        .or_else(|| runtime.as_ref().map(|runtime| runtime.module.clone()))
        .context("Módulo ausente")?;
    let helper = helper
        .or_else(|| runtime.map(|runtime| runtime.helper))
        .context("Auxiliar ausente")?;
    let state = state
        .or_else(|| env::var_os("DESKTOPID_STATE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("desktopid-linux")
        });
    Ok((module, helper, state))
}

/// An explicit read-only inventory never needs an installed signing helper.
pub fn inventory_paths(
    module: Option<PathBuf>,
    state: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let home = PathBuf::from(env::var_os("HOME").context("HOME ausente")?);
    let module = match module {
        Some(module) => module,
        None => desktopid_client::installation::runtime::select(&home)?.module,
    };
    let state = state
        .or_else(|| env::var_os("DESKTOPID_STATE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("desktopid-linux")
        });
    Ok((module, state))
}

pub fn run(module: &Path, helper: &Path, state: &Path, number: usize) -> Result<()> {
    ensure!(
        std::io::stdin().is_terminal(),
        "Execute no terminal interativo"
    );
    ensure!(
        env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            || env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
        "Sessão gráfica indisponível; nenhuma assinatura solicitada"
    );
    ensure!(
        state.is_absolute(),
        "Diretório da identidade deve ser absoluto"
    );
    check_owned_private(state, true)?;
    let state = state.canonicalize()?;
    ensure!(
        !state.starts_with("/mnt"),
        "Guarde identidade e artefatos no sistema de arquivos Linux"
    );
    // Public wallet validation does not load the installation's private key here.
    let wallet = Store::new(state.clone())?.load_wallet_public()?;
    let certificate = &wallet
        .get(
            number
                .checked_sub(1)
                .context("Número de certificado inexistente")?,
        )
        .context("Número de certificado inexistente")?
        .certificate;
    let identifier = publication::digest(&certificate.to_der()?);
    let parent = state.join("pkcs11-tests");
    if !parent.try_exists()? {
        fs::DirBuilder::new().mode(0o700).create(&parent)?;
    }
    check_owned_private(&parent, true)?;
    let mut nonce = [0; 16];
    openssl::rand::rand_bytes(&mut nonce)?;
    let directory = parent.join(nonce.iter().map(|b| format!("{b:02x}")).collect::<String>());
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let document = b"DesktopID Linux - teste real via PKCS11/OpenSC e autorizacao local.\n";
    write_new_private(&directory.join("document.txt"), document)?;
    write_new_private(&directory.join("certificate.pem"), &certificate.to_pem()?)?;
    write_new_private(
        &directory.join("attempt.json"),
        &serde_json::to_vec(
            &json!({"operation":"pkcs11-laboratory-signature","certificate_number":number,"result":"requires-review"}),
        )?,
    )?;
    println!("Será solicitada UMA assinatura de texto de laboratório pelo módulo PKCS11.");
    println!(
        "A janela local pedirá senha do aplicativo Certisign e código do celular. Cancelar não solicita assinatura."
    );
    println!("Artefatos privados: {}", directory.display());
    std::io::stdout().flush()?;
    let signature = directory.join("signature.bin");
    let mut command = Command::new("pkcs11-tool");
    command
        .arg("--module")
        .arg(module)
        .args([
            "--slot",
            "8",
            "--login",
            "--sign",
            "--mechanism",
            "SHA256-RSA-PKCS",
            "--id",
            &identifier,
        ])
        .arg("--input-file")
        .arg(directory.join("document.txt"))
        .arg("--output-file")
        .arg(&signature)
        .env("DESKTOPID_STATE_DIR", &state)
        .env("DESKTOPID_HELPER", helper);
    // SAFETY: umask is async-signal-safe and runs only in the freshly forked child.
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            Ok(())
        });
    }
    let result = lab::capture(&mut command, Duration::from_secs(430), b"")
        .map_err(|_| anyhow::anyhow!("Tempo ou execução do teste falhou; artefatos preservados em {}. Conferir antes de nova assinatura; nenhuma repetição automática.", directory.display()))?;
    if !result.status.success() {
        let safe = if String::from_utf8_lossy(&result.stderr).contains("CKR_FUNCTION_CANCELED") {
            "Operação cancelada"
        } else {
            "Operação PKCS11 não foi confirmada"
        };
        anyhow::bail!(
            "{safe}; artefatos preservados em {}. Não houve repetição automática.",
            directory.display()
        );
    }
    check_owned_private(&signature, false)?;
    let public = certificate.public_key()?;
    verify_digest(
        &public,
        Algorithm::Sha256,
        &hash(MessageDigest::sha256(), document)?,
        &fs::read(&signature)?,
    )?;
    write_new_private(
        &directory.join("result.json"),
        &serde_json::to_vec(
            &json!({"scope":"real-pkcs11-laboratory","certificate_number":number,"signature_verified":true}),
        )?,
    )?;
    println!("Assinatura real via PKCS11/OpenSC aceita e verificada no Linux.");
    println!("Artefatos privados salvos em {}", directory.display());
    Ok(())
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    #[test]
    fn explicit_inventory_does_not_require_a_default_module_or_signing_helper() {
        let temporary = tempfile::tempdir().unwrap();
        let module = temporary.path().join("laboratory-provider.so");
        let state = temporary.path().join("laboratory-state");
        let selected = inventory_paths(Some(module.clone()), Some(state.clone())).unwrap();
        assert_eq!(selected, (module, state));
        assert!(fs::read_dir(temporary.path()).unwrap().next().is_none());
    }
}
