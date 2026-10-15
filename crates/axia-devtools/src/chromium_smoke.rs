//! Disposable-container TLS laboratory; no browser profile or trust on host.
use crate::{lab, lab_helper};
use anyhow::{Result, ensure};
use openssl::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::Rsa,
    ssl::{SslAcceptor, SslMethod, SslVerifyMode, SslVersion},
    x509::{
        X509, X509NameBuilder,
        extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
    },
};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn server_certificates() -> Result<(X509, PKey<Private>, X509)> {
    let ca_key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let mut ca_name = X509NameBuilder::new()?;
    ca_name.append_entry_by_text("CN", "Axia Chromium Laboratory CA")?;
    let ca_name = ca_name.build();
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text("CN", "localhost")?;
    let name = name.build();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let before = Asn1Time::from_unix(now - 60)?;
    let after = Asn1Time::from_unix(now + 86400)?;
    let mut ca = X509::builder()?;
    ca.set_version(2)?;
    let serial = Asn1Integer::from_bn(BigNum::from_u32(1)?.as_ref())?;
    ca.set_serial_number(&serial)?;
    ca.set_subject_name(&ca_name)?;
    ca.set_issuer_name(&ca_name)?;
    ca.set_pubkey(&ca_key)?;
    ca.set_not_before(&before)?;
    ca.set_not_after(&after)?;
    ca.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    ca.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;
    ca.sign(&ca_key, MessageDigest::sha256())?;
    let ca = ca.build();
    let mut leaf = X509::builder()?;
    leaf.set_version(2)?;
    let serial = Asn1Integer::from_bn(BigNum::from_u32(2)?.as_ref())?;
    leaf.set_serial_number(&serial)?;
    leaf.set_subject_name(&name)?;
    leaf.set_issuer_name(&ca_name)?;
    leaf.set_pubkey(&key)?;
    leaf.set_not_before(&before)?;
    leaf.set_not_after(&after)?;
    leaf.append_extension(
        SubjectAlternativeName::new()
            .dns("localhost")
            .build(&leaf.x509v3_context(Some(&ca), None))?,
    )?;
    leaf.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
    leaf.sign(&ca_key, MessageDigest::sha256())?;
    Ok((ca, key, leaf.build()))
}

pub fn smoke(root: &Path, client: &Path, module: &Path, no_sandbox: bool) -> Result<()> {
    lab::require_container()?;
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Execute Chromium como usuário normal do contêiner"
    );
    let fixture = lab::fixture(root)?;
    let certificate = X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())?;
    let client = client.canonicalize()?;
    let module = module.canonicalize()?;
    let home = tempfile::Builder::new()
        .prefix("axia-chromium-lab-")
        .tempdir()?;
    let home = home.path();
    lab_helper::mark(home)?;
    let state = home.join(".config/desktopid-linux");
    lab::initialize(&client, home, &state, &fixture)?;
    let installed = home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so");
    fs::create_dir_all(installed.parent().unwrap())?;
    fs::copy(module, &installed)?;
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o700))?;
    let helper = lab_helper::install(home)?;
    let command = |binary: &Path| {
        let mut command = lab::command(binary, home);
        command
            .env("DESKTOPID_STATE_DIR", &state)
            .env("DESKTOPID_HELPER", &helper)
            .env("AXIA_LAB_HOME", home);
        command
    };
    let inventory = lab::successful(command(&client).args(["setup-browsers", "--list"]))?;
    let inventory: Value = serde_json::from_slice(&inventory.stdout)?;
    ensure!(
        inventory["installed"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Chromium")),
        "Chromium nativo não detectado"
    );
    let reports = lab::successful(command(&client).args([
        "setup-browsers",
        "--yes",
        "--family",
        "chromium",
    ]))?;
    let reports: Value = serde_json::from_slice(&reports.stdout)?;
    ensure!(
        reports.as_array().unwrap().len() == 1
            && reports[0]["certificate_count"] == 1
            && !home.join("calls.jsonl").exists(),
        "Configuração Chromium assinou ou perdeu certificado"
    );
    let (authority, key, leaf) = server_certificates()?;
    let ca = home.join("laboratory-ca.pem");
    lab::private_write(&ca, &authority.to_pem()?)?;
    lab::successful(
        command(Path::new("certutil"))
            .args(["-A", "-d"])
            .arg(format!("sql:{}", home.join(".pki/nssdb").display()))
            .args(["-n", "Axia Laboratory CA", "-t", "C,,", "-i"])
            .arg(&ca),
    )?;
    let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls_server())?;
    acceptor.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    acceptor.set_max_proto_version(Some(SslVersion::TLS1_2))?;
    acceptor.set_certificate(&leaf)?;
    acceptor.set_private_key(&key)?;
    acceptor.check_private_key()?;
    acceptor.cert_store_mut().add_cert(certificate.clone())?;
    acceptor.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
    let acceptor = Arc::new(acceptor.build());
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let stop = stopped.clone();
    let authenticated = count.clone();
    let peer_der = certificate.to_der()?;
    let server = thread::spawn(move || {
        let mut connections = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            if let Ok((stream, _)) = listener.accept() {
                let acceptor = acceptor.clone();
                let authenticated = authenticated.clone();
                let expected = peer_der.clone();
                connections.push(thread::spawn(move || -> Result<()> {
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                    let mut stream = acceptor.accept(stream).map_err(|_| anyhow::anyhow!("TLS local não autenticou"))?;
                    ensure!(stream.ssl().peer_certificate().is_some_and(|c| c.to_der().is_ok_and(|d| d == expected)) && stream.ssl().version_str() == "TLSv1.2", "Certificado/TLS do navegador incorreto");
                    let mut request = [0;4096]; let size = stream.read(&mut request)?;
                    if !request[..size].starts_with(b"GET ") { return Ok(()); }
                    authenticated.fetch_add(1,Ordering::SeqCst);
                    let body = b"<html><body>AXIA_CLIENT_CERTIFICATE_OK</body></html>";
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len())?;
                    stream.write_all(body)?; stream.flush()?; let _ = stream.shutdown();
                    Ok(())
                }));
                if connections.len() >= 64 {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
        for connection in connections {
            let _ = connection.join();
        }
    });
    let before = lab::snapshot(&state)?;
    let executable = ["/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or_else(|| anyhow::anyhow!("Chromium ausente"));
    let result = (|| {
        let executable = executable?;
        let mut browser = command(Path::new(executable));
        browser
            .args([
                "--headless=new",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
            ])
            .arg(format!(
                "--user-data-dir={}",
                home.join("chromium-profile").display()
            ))
            .arg("--dump-dom")
            .arg(format!("https://localhost:{port}/"));
        if no_sandbox {
            browser.arg("--no-sandbox");
        }
        let output = lab::capture(&mut browser, Duration::from_secs(40), b"")?;
        ensure!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("AXIA_CLIENT_CERTIFICATE_OK"),
            "Chromium não autenticou no servidor de laboratório: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            count.load(Ordering::SeqCst) > 0 && home.join("calls.jsonl").exists(),
            "Chromium não assinou ou servidor não validou certificado"
        );
        ensure!(
            lab::snapshot(&state)? == before,
            "Navegador alterou identidade/carteira"
        );
        Ok(())
    })();
    stopped.store(true, Ordering::SeqCst);
    server
        .join()
        .map_err(|_| anyhow::anyhow!("Servidor TLS falhou"))?;
    result?;
    println!(
        "Chromium: native discovery, NSS setup, client certificate selection and TLS 1.2 signature verified by loopback server; synthetic helper only; sandbox={}",
        !no_sandbox
    );
    Ok(())
}
