//! One-shot external helper: credentials collected locally, never through IPC.
use crate::{
    client::Api,
    protocol::{Algorithm, CertificateReference, reference_from_key_name, verify_digest},
    signing::validate_certificate,
    storage::Store,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::{
    io::{self, Read},
    process::{Command, Stdio},
};
use zeroize::Zeroizing;

#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Autorização cancelada; nenhuma assinatura solicitada")
    }
}
impl std::error::Error for Cancelled {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    certificate_sha256: String,
    algorithm: String,
    digest_base64: String,
}
fn dialog(label: &str) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    ensure!(
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        "Sessão gráfica indisponível; nenhuma autorização ou assinatura solicitada"
    );
    let label = label
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let text = format!(
        "Um aplicativo deste computador solicitou uma assinatura digital.\nCertificado: {label}\nAutorize somente se você iniciou essa operação."
    );
    let result = Command::new("/usr/bin/zenity")
        .args(["--forms", "--title=Autorizar certificado digital", "--text"])
        .arg(text)
        .arg("--add-password=Senha do aplicativo Certisign")
        .arg("--add-password=Código do celular (OTP)")
        .arg("--separator=\n")
        .arg("--timeout=180")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("Não foi possível abrir a janela de autorização; nenhuma assinatura solicitada")?;
    if !result.status.success() {
        return Err(Cancelled.into());
    }
    let raw = Zeroizing::new(result.stdout);
    let text = std::str::from_utf8(&raw).context("Entrada da janela inválida")?;
    let mut parts = text.strip_suffix('\n').unwrap_or(text).split('\n');
    let password = Zeroizing::new(parts.next().context("Senha ausente")?.to_owned());
    let otp = Zeroizing::new(parts.next().context("OTP ausente")?.to_owned());
    ensure!(
        parts.next().is_none() && !password.is_empty() && !otp.is_empty(),
        "Preencha senha e código; nenhuma assinatura solicitada"
    );
    Ok((password, otp))
}

pub fn run(store: &Store) -> Result<()> {
    let mut input = Vec::new();
    io::stdin().take(8193).read_to_end(&mut input)?;
    ensure!(input.len() <= 8192, "Pedido local excede o limite");
    let request: Request = serde_json::from_slice(&input).context("Pedido local inválido")?;
    ensure!(
        request.schema_version == 1,
        "Versão de pedido local inválida"
    );
    ensure!(
        request.certificate_sha256.len() == 64
            && request
                .certificate_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Impressão digital inválida"
    );
    let algorithm = match request.algorithm.as_str() {
        "SHA256" => Algorithm::Sha256,
        "SHA384" => Algorithm::Sha384,
        "SHA512" => Algorithm::Sha512,
        _ => anyhow::bail!("Algoritmo não suportado"),
    };
    let digest = STANDARD
        .decode(&request.digest_base64)
        .context("Digest local inválido")?;
    ensure!(
        digest.len() == algorithm.digest_len(),
        "Tamanho de digest inválido"
    );
    let (config, identity) = store.load()?;
    let wallet = store.load_wallet()?;
    let matches = wallet
        .iter()
        .filter(|entry| {
            entry
                .certificate
                .digest(openssl::hash::MessageDigest::sha256())
                .is_ok_and(|v| {
                    v.iter().map(|b| format!("{b:02x}")).collect::<String>()
                        == request.certificate_sha256
                })
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Pedido não identifica um único certificado da carteira atual"
    );
    let entry = matches[0];
    validate_certificate(
        entry,
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?,
    )?;
    let (serial, issue) = reference_from_key_name(&entry.key_name)?;
    let desktop = config
        .desktop_code
        .as_deref()
        .context("Instalação não cadastrada")?;
    let label = entry
        .certificate
        .subject_name()
        .entries_by_nid(openssl::nid::Nid::COMMONNAME)
        .next()
        .and_then(|e| e.data().to_string().ok())
        .unwrap_or_else(|| "Certificado selecionado".into());
    let (password, otp) = dialog(&label)?;
    let api = Api::new(&config.server)?;
    let reference = CertificateReference {
        serial: &serial,
        issue: &issue,
    };
    let token = api.open_session(desktop, &reference, &password, &otp, false, &identity)?;
    let signature = api.sign_digest(desktop, &reference, &token, algorithm, &digest, &identity)?;
    let public = entry.certificate.public_key()?;
    verify_digest(&public, algorithm, &digest, &signature)?;
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"signature_base64":STANDARD.encode(signature)})
    );
    Ok(())
}
