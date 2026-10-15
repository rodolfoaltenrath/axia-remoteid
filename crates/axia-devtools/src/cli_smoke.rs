use crate::{lab, publication};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::x509::X509;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};

pub fn smoke(root: &Path, binary: &Path, report: &Path) -> Result<()> {
    let binary = binary.canonicalize()?;
    let fixture = lab::fixture(root)?;
    let temp = tempfile::Builder::new().prefix("axia-cli-lab-").tempdir()?;
    let home = temp.path();
    let state = home.join("identity");
    let missing = home.join("must-not-create-state");
    let mut checks = Vec::new();
    let run = |state: &Path, args: &[&str], success: bool| -> Result<std::process::Output> {
        let output = lab::capture(
            lab::command(&binary, home)
                .arg("--state-dir")
                .arg(state)
                .args(args),
            Duration::from_secs(30),
            b"",
        )?;
        ensure!(
            output.status.success() == success,
            "CLI: resultado inesperado em {}: {}",
            args.first().unwrap_or(&"gui"),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    };
    let init = [
        "init",
        "--server",
        "https://example.invalid",
        "--access-url",
        "laboratory",
    ];
    run(&state, &init, true)?;
    let original = fs::read(state.join("identity.pem"))?;
    ensure!(
        lab::mode(&state)? == 0o700 && lab::mode(&state.join("identity.pem"))? == 0o600,
        "Permissões da identidade de laboratório incorretas"
    );
    run(&state, &init, false)?;
    ensure!(
        fs::read(state.join("identity.pem"))? == original,
        "init substituiu identidade existente"
    );
    let status = run(&state, &["status"], true)?;
    let mut config: Value = serde_json::from_slice(&status.stdout)?;
    ensure!(
        config["desktop_code"].is_null()
            && !String::from_utf8_lossy(&status.stdout).contains("PRIVATE KEY"),
        "status expôs identidade privada"
    );
    checks.extend([
        "local-init-private-permissions",
        "init-does-not-replace-key",
        "status-does-not-print-private-key",
    ]);
    let cert = home.join("lab.pem");
    lab::private_write(
        &cert,
        fixture["certificate_pem"].as_str().unwrap().as_bytes(),
    )?;
    let document = home.join("document.bin");
    let document_bytes = lab::decode(&fixture["document_base64"])?;
    let signature = home.join("signature.bin");
    lab::private_write(&document, &document_bytes)?;
    for (algorithm, check) in [
        ("sha256", "sha256-python-signature-and-tamper-rejection"),
        ("sha384", "sha384-python-signature-and-tamper-rejection"),
        ("sha512", "sha512-python-signature-and-tamper-rejection"),
    ] {
        lab::private_write(
            &signature,
            &lab::decode(&fixture["signatures"][algorithm.to_uppercase()])?,
        )?;
        let args = [
            "verify",
            "--file",
            document.to_str().unwrap(),
            "--certificate",
            cert.to_str().unwrap(),
            "--signature",
            signature.to_str().unwrap(),
            "--algorithm",
            algorithm,
        ];
        run(&state, &args, true)?;
        lab::private_write(&document, b"changed laboratory document")?;
        run(&state, &args, false)?;
        lab::private_write(&document, &document_bytes)?;
        checks.push(check);
    }
    run(
        &state,
        &[
            "verify",
            "--file",
            document.to_str().unwrap(),
            "--certificate",
            cert.to_str().unwrap(),
            "--signature",
            signature.to_str().unwrap(),
            "--algorithm",
            "sha1",
        ],
        false,
    )?;
    checks.push("unsupported-sha1-rejected-before-operation");
    for command in [vec![], vec!["gui"]] {
        let result = run(&missing, &command, false)?;
        ensure!(
            String::from_utf8_lossy(&result.stderr).contains("Sessão gráfica indisponível")
                && !missing.exists(),
            "GUI headless criou estado ou mudou erro"
        );
    }
    checks.push("gui-default-and-explicit-command-reject-headless-session-without-state");
    run(&missing, &["autostart", "--enable"], true)?;
    let entry = home.join(".config/autostart/axia-remotedid.desktop");
    let enabled = fs::read_to_string(&entry)?;
    ensure!(
        enabled.contains("Name=Axia RemotedID") && enabled.contains("Hidden=false"),
        "Autostart não habilitou entrada Axia"
    );
    run(&missing, &["autostart", "--disable"], true)?;
    ensure!(
        fs::read_to_string(&entry)?.contains("Hidden=true"),
        "Autostart não desabilitou entrada"
    );
    ensure!(
        String::from_utf8_lossy(&run(&missing, &["autostart"], true)?.stdout)
            .contains("desativada"),
        "Estado autostart incorreto"
    );
    run(&missing, &["startup"], true)?;
    ensure!(
        !missing.exists() && fs::read(state.join("identity.pem"))? == original,
        "Startup desabilitado alterou conta"
    );
    checks.push("autostart-enable-disable-and-disabled-startup-without-account-or-network");
    let broker = run(&missing, &["pkcs11-broker"], true)?;
    ensure!(
        broker.stdout.is_empty() && !missing.exists(),
        "Broker EOF criou estado ou escreveu resposta"
    );
    checks.push("signing-broker-closes-on-parent-pipe-eof-without-state-or-network");
    for (args, message, check) in [
        (
            vec!["login-check", "--server", "http://example.invalid"],
            "HTTPS",
            "login-check-rejects-http-before-credentials",
        ),
        (
            vec!["login-check", "--email", "lab@example.invalid"],
            "terminal interativo",
            "login-check-requires-local-terminal-and-creates-no-state",
        ),
        (
            vec!["connect"],
            "terminal interativo",
            "connect-requires-interactive-terminal-before-creating-identity",
        ),
    ] {
        let result = run(&missing, &args, false)?;
        ensure!(
            String::from_utf8_lossy(&result.stderr).contains(message) && !missing.exists(),
            "CLI pediu operação sem terminal ou criou estado"
        );
        checks.push(check);
    }
    config["desktop_code"] = json!("lab-desktop");
    lab::json_write(&state.join("config.json"), &config)?;
    let certificate = X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())?;
    lab::json_write(
        &state.join("wallet.json"),
        &json!({"schema_version":1,"public_key_sha256":config["public_key_sha256"],"desktop_code":"lab-desktop","certificados":[{"index":0,"keyName":"LAB-SERIAL;LAB-ISSUER","base64":STANDARD.encode(certificate.to_der()?)}]}),
    )?;
    let listing = run(&state, &["sign-test", "--list"], true)?;
    let listing = String::from_utf8_lossy(&listing.stdout);
    ensure!(
        listing.contains("RSA")
            && listing.contains("apto ao teste")
            && !["LAB-SERIAL", "LAB-ISSUER", "PRIVATE KEY"]
                .iter()
                .any(|s| listing.contains(s)),
        "Listagem expôs dados privados ou perdeu certificado"
    );
    let result = run(&state, &["sign-test"], false)?;
    ensure!(
        String::from_utf8_lossy(&result.stderr).contains("terminal interativo")
            && !state.join("signing-tests").exists()
            && fs::read(state.join("identity.pem"))? == original,
        "sign-test não interativo criou assinatura ou mudou identidade"
    );
    checks.extend([
        "sign-test-lists-cached-certificates-without-account-or-personal-reference",
        "sign-test-requires-interactive-terminal-before-signature-artifacts",
    ]);
    let run_id = "0123456789abcdef0123456789abcdef";
    let artifacts = state.join("signing-tests").join(run_id);
    fs::create_dir_all(&artifacts)?;
    fs::set_permissions(
        artifacts.parent().unwrap(),
        fs::Permissions::from_mode(0o700),
    )?;
    fs::set_permissions(&artifacts, fs::Permissions::from_mode(0o700))?;
    lab::json_write(
        &artifacts.join("attempt.json"),
        &json!({"operation":"laboratory-signature","algorithm":"SHA256","certificate_number":1}),
    )?;
    lab::private_write(&artifacts.join("document.txt"), b"tampered document")?;
    lab::private_write(
        &artifacts.join("certificate.pem"),
        fixture["certificate_pem"].as_str().unwrap().as_bytes(),
    )?;
    let candidate = lab::decode(&fixture["signatures"]["SHA256"])?;
    lab::private_write(&artifacts.join("signature-candidate.bin"), &candidate)?;
    let args = ["recover-sign-test", "--run-id", run_id];
    run(&state, &args, false)?;
    ensure!(
        !artifacts.join("result.json").exists(),
        "Recuperação aceitou documento alterado"
    );
    lab::private_write(&artifacts.join("document.txt"), &document_bytes)?;
    let recovered = run(&state, &args, true)?;
    let result: Value = serde_json::from_slice(&fs::read(artifacts.join("result.json"))?)?;
    ensure!(
        String::from_utf8_lossy(&recovered.stdout).contains("Nenhuma operação de rede")
            && result["recovered_offline"] == true
            && fs::read(artifacts.join("signature.bin"))? == candidate
            && lab::mode(&artifacts.join("signature.bin"))? == 0o600,
        "Recuperação offline ou permissões incorretas"
    );
    run(&state, &args, false)?;
    run(
        &state,
        &["recover-sign-test", "--run-id", "../identity"],
        false,
    )?;
    checks.extend([
        "recover-sign-test-verifies-offline-and-rejects-tampered-document",
        "recover-sign-test-preserves-existing-results-and-rejects-path-traversal",
    ]);
    let binary_bytes = fs::read(binary)?;
    let mut result = json!({"schema_version":1,"successful":true,"scope":"rust-laboratory-client","system":"Linux","binary_is_elf":binary_bytes.starts_with(b"\x7fELF"),"binary_sha256":publication::digest(&binary_bytes),"checks":checks,"cloud_login_tested":false,"cloud_registration_tested":false,"cloud_signature_tested":false,"pkcs11_provider_tested":false,"gui_tested":false});
    // Only the validation controller sets these after its own checks passed.
    if std::env::var("DESKTOPID_RUST_VALIDATION").as_deref() == Ok("passed") {
        for name in [
            "cargo_fmt_check_passed",
            "cargo_clippy_passed",
            "cargo_test_passed",
            "cargo_build_passed",
        ] {
            result[name] = json!(true);
        }
        result["rust_tests_passed"] = json!(
            std::env::var("DESKTOPID_RUST_TESTS")
                .unwrap_or_default()
                .parse::<u64>()?
        );
    }
    lab::report(report, &result)?;
    println!(
        "Linux executable: {} checks passed; report: {}",
        checks.len(),
        report.display()
    );
    Ok(())
}
