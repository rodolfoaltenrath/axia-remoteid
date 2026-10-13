//! SHA-256 laboratory signing. This produces a detached RSA signature, not a PDF container.
use crate::{
    client::{Api, WalletCertificate},
    identity::Identity,
    protocol::{
        Algorithm, CertificateReference, decode_signature, reference_from_key_name,
        signature_response_summary, verify_digest,
    },
    storage::{Store, check_owned_private, read_private, write_new_private},
};
use anyhow::{Result, ensure};
use openssl::{asn1::Asn1Time, hash::hash};
use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

pub const LAB_DOCUMENT: &[u8] = b"DesktopID Linux - teste de assinatura SHA-256.\nDocumento sintetico de laboratorio para verificar interoperabilidade.\n";

pub fn validate_certificate(entry: &WalletCertificate, unix_seconds: i64) -> Result<()> {
    reference_from_key_name(&entry.key_name)?;
    ensure!(
        entry.certificate.public_key()?.rsa().is_ok(),
        "Esta versão suporta somente certificados RSA"
    );
    let now = Asn1Time::from_unix(unix_seconds)?;
    ensure!(
        entry.certificate.not_before().compare(&now)? != Ordering::Greater,
        "Certificado ainda não está válido"
    );
    ensure!(
        entry.certificate.not_after().compare(&now)? != Ordering::Less,
        "Certificado expirado; selecione outro certificado"
    );
    Ok(())
}

pub struct SigningAuthorization<'a> {
    pub password: &'a str,
    pub otp: &'a str,
    pub push: bool,
}

/// No login, registration, wallet refresh, token persistence or automatic retry.
/// Certificate validation happens before contacting any signature endpoint.
pub fn sign_and_verify(
    api: &Api,
    identity: &Identity,
    desktop: &str,
    entry: &WalletCertificate,
    document: &[u8],
    authorization: &SigningAuthorization<'_>,
) -> Result<Vec<u8>> {
    sign_and_verify_recorded(api, identity, desktop, entry, document, authorization, None)
}

/// Preserve the decoded candidate before cryptographic verification. The
/// candidate alone is never a successful result; it permits offline recovery.
pub fn sign_and_verify_recorded(
    api: &Api,
    identity: &Identity,
    desktop: &str,
    entry: &WalletCertificate,
    document: &[u8],
    authorization: &SigningAuthorization<'_>,
    artifacts: Option<&Path>,
) -> Result<Vec<u8>> {
    if let Some(directory) = artifacts {
        check_owned_private(directory, true)?;
        ensure!(
            !directory.join("signature-candidate.bin").try_exists()?
                && !directory
                    .join("signature-response-summary.json")
                    .try_exists()?,
            "Já existe resposta neste teste; verificar localmente antes de outra operação"
        );
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    validate_certificate(entry, i64::try_from(now)?)?;
    let (serial, issue) = reference_from_key_name(&entry.key_name)?;
    let reference = CertificateReference {
        serial: &serial,
        issue: &issue,
    };
    let digest = hash(Algorithm::Sha256.message_digest(), document)?;
    let token = api.open_session(
        desktop,
        &reference,
        authorization.password,
        authorization.otp,
        authorization.push,
        identity,
    )?;
    let response = api.request_digest_signature(
        desktop,
        &reference,
        &token,
        Algorithm::Sha256,
        &digest,
        identity,
    )?;
    let summary = signature_response_summary(&response);
    if let Some(directory) = artifacts {
        write_new_private(
            &directory.join("signature-response-summary.json"),
            &serde_json::to_vec_pretty(&summary)?,
        )?;
    }
    let signature = decode_signature(&response)?;
    if let Some(directory) = artifacts {
        write_new_private(&directory.join("signature-candidate.bin"), &signature)?;
        println!(
            "Resposta de assinatura (metadados): {}",
            serde_json::to_string(&summary)?
        );
    }
    let public = entry.certificate.public_key()?;
    verify_digest(&public, Algorithm::Sha256, &digest, &signature)?;
    Ok(signature)
}

/// Entirely offline: no Api construction, credentials, session or signature call.
pub fn recover_signing_test(store: &Store, run_id: &str) -> Result<PathBuf> {
    let directory = store.existing_signing_test_dir(run_id)?;
    ensure!(
        !directory.join("signature.bin").try_exists()?
            && !directory.join("result.json").try_exists()?,
        "Este teste já possui resultado/assinatura final; arquivos preservados"
    );
    let attempt: serde_json::Value =
        serde_json::from_slice(&read_private(&directory.join("attempt.json"))?)?;
    ensure!(
        attempt["operation"] == "laboratory-signature" && attempt["algorithm"] == "SHA256",
        "Marcador não corresponde a um teste de assinatura SHA-256"
    );
    let document = read_private(&directory.join("document.txt"))?;
    let cert = openssl::x509::X509::from_pem(&read_private(&directory.join("certificate.pem"))?)?;
    let signature = read_private(&directory.join("signature-candidate.bin"))?;
    let digest = hash(Algorithm::Sha256.message_digest(), &document)?;
    let public = cert.public_key()?;
    verify_digest(&public, Algorithm::Sha256, &digest, &signature)?;
    write_new_private(&directory.join("signature.bin"), &signature)?;
    write_new_private(
        &directory.join("result.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "operation":"laboratory-signature", "algorithm":"SHA256", "signature_verified":true,
            "certificate_number":attempt["certificate_number"], "recovered_offline":true,
            "verification_scope":"RSA signature matches saved document and certificate; no PKI trust validation"
        }))?,
    )?;
    Ok(directory)
}
