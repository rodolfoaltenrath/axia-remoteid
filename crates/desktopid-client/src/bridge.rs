//! Local signing helper; reusable capabilities remain in helper memory only.
use crate::{
    client::Api,
    protocol::{Algorithm, CertificateReference, reference_from_key_name, verify_digest},
    session_cache::{Clock, SessionCache},
    signing::validate_certificate,
    storage::Store,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
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
pub(crate) struct Request {
    schema_version: u32,
    certificate_sha256: String,
    algorithm: String,
    digest_base64: String,
}
fn dialog(label: &str, cached: bool) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    ensure!(
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        "Sessão gráfica indisponível; nenhuma autorização ou assinatura solicitada"
    );
    let label = label
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let authorization = if cached {
        "Esta autorização permitirá novas assinaturas com este certificado neste aplicativo por até 10 minutos.\nFechar o aplicativo encerra a autorização. Senha e código não serão guardados."
    } else {
        "Autorize somente se você iniciou essa operação."
    };
    let text = format!(
        "Um aplicativo deste computador solicitou uma assinatura digital.\nCertificado: {label}\n{authorization}"
    );
    let result = Command::new("/usr/bin/zenity")
        .args([
            "--forms",
            "--title=Axia RemotedID — Autorizar certificado",
            "--text",
        ])
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

pub(crate) fn sign_request(
    api: &Api,
    store: &Store,
    request: Request,
    cache: &mut SessionCache,
    cached: bool,
    mut authorize: impl FnMut(&str) -> Result<(Zeroizing<String>, Zeroizing<String>)>,
) -> Result<Vec<u8>> {
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
    let reference = CertificateReference {
        serial: &serial,
        issue: &issue,
    };
    let key = (
        config.server.clone(),
        desktop.to_owned(),
        config.public_key_sha256.clone(),
        request.certificate_sha256,
        entry.key_name.clone(),
    );
    cache.retain_context(&key);
    let previous = if cached {
        cache.get(&key, Clock::now())
    } else {
        None
    };
    let new_session = previous.is_none();
    let (token, created) = match previous {
        Some(token) => (token, None),
        None => {
            let (password, otp) = authorize(&label)?;
            let token = api.open_session(desktop, &reference, &password, &otp, false, &identity)?;
            ensure!(token.len() <= 16384, "Token de sessão excede o limite");
            (token, Some(Clock::now()))
        }
    };
    let result = api
        .sign_digest(desktop, &reference, &token, algorithm, &digest, &identity)
        .and_then(|signature| {
            let public = entry.certificate.public_key()?;
            verify_digest(&public, algorithm, &digest, &signature)?;
            Ok(signature)
        });
    match result {
        Ok(signature) => {
            if cached && new_session {
                cache.put(key, token, created.context("Tempo da autorização ausente")?);
            }
            Ok(signature)
        }
        Err(error) => {
            cache.remove(&key);
            Err(error.context(
                "Assinatura não confirmada; autorização encerrada, sem repetição automática",
            ))
        }
    }
}

pub fn run(store: &Store) -> Result<()> {
    let mut input = Vec::new();
    io::stdin().take(8193).read_to_end(&mut input)?;
    ensure!(input.len() <= 8192, "Pedido local excede o limite");
    let request = serde_json::from_slice(&input).context("Pedido local inválido")?;
    let api = Api::new(&store.load()?.0.server)?;
    let signature = sign_request(
        &api,
        store,
        request,
        &mut SessionCache::default(),
        false,
        |label| dialog(label, false),
    )?;
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"signature_base64":STANDARD.encode(signature)})
    );
    Ok(())
}

enum Input {
    Request(Vec<u8>),
    Expired,
    Eof,
}

fn read_request(idle: std::time::Duration) -> Result<Input> {
    let input = io::stdin();
    let deadline = std::time::Instant::now() + idle;
    let wall_started = std::time::SystemTime::now();
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero()
            || wall_started
                .elapsed()
                .map(|elapsed| elapsed >= idle)
                .unwrap_or(true)
        {
            return Ok(Input::Expired);
        }
        let mut descriptor = libc::pollfd {
            fd: input.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid descriptor and bounded millisecond timeout.
        let status = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                remaining.as_millis().clamp(1, 1000) as i32,
            )
        };
        if status < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if status == 0 {
            continue;
        }
        let mut byte = [0u8];
        // SAFETY: byte is a writable one-byte buffer; EOF closes the broker.
        let read = unsafe { libc::read(input.as_raw_fd(), byte.as_mut_ptr().cast(), 1) };
        if read == 0 {
            return Ok(Input::Eof);
        }
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if byte[0] == b'\n' {
            return Ok(Input::Request(bytes));
        }
        ensure!(bytes.len() < 8192, "Pedido local excede o limite");
        bytes.push(byte[0]);
    }
}

/// One pipe-connected helper per application. Only public requests/signatures
/// cross IPC. Closing its parent pipes or idle expiry destroys all tokens.
pub fn run_broker(store: &Store) -> Result<()> {
    let mut cache = SessionCache::default();
    let mut first = true;
    loop {
        let idle = match cache.next_expiration(Clock::now()) {
            Some(remaining) => remaining,
            None if first => std::time::Duration::from_secs(400),
            None => return Ok(()),
        };
        let input = match read_request(idle)? {
            Input::Request(input) => input,
            Input::Expired if first => return Ok(()),
            Input::Expired => continue,
            Input::Eof => return Ok(()),
        };
        first = false;
        let result = (|| {
            let request = serde_json::from_slice(&input).context("Pedido local inválido")?;
            let api = Api::new(&store.load()?.0.server)?;
            sign_request(&api, store, request, &mut cache, true, |label| {
                dialog(label, true)
            })
        })();
        let failed = result.is_err();
        let response = match result {
            Ok(signature) => {
                serde_json::json!({"schema_version":1,"signature_base64":STANDARD.encode(signature)})
            }
            Err(error) => {
                serde_json::json!({"schema_version":1,"error":if error.downcast_ref::<Cancelled>().is_some() { "cancelled" } else { "failed" }})
            }
        };
        writeln!(io::stdout(), "{response}")?;
        io::stdout().flush()?;
        if failed {
            return Ok(());
        }
    }
}
