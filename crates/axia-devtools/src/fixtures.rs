//! Explicit generation of PUBLIC, self-signed lab material; never a real identity.
use crate::publication;
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use desktopid_client::{
    identity::Identity,
    protocol::{Algorithm, flatten_values, verify_digest},
};
use openssl::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    hash::{MessageDigest, hash},
    pkey::PKey,
    rsa::{Padding, Rsa},
    sign::Signer,
    x509::{X509, X509NameBuilder},
};
use serde_json::json;
use std::{fs, io::Write, path::Path};

fn vectors() -> Result<serde_json::Value> {
    let key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text("CN", "DesktopID laboratory only")?;
    let name = name.build();
    let mut cert = X509::builder()?;
    cert.set_version(2)?;
    let number = BigNum::from_u32(123456)?;
    let serial = Asn1Integer::from_bn(&number)?;
    cert.set_serial_number(&serial)?;
    cert.set_subject_name(&name)?;
    cert.set_issuer_name(&name)?;
    cert.set_pubkey(&key)?;
    let before = Asn1Time::from_unix(1_577_836_800)?;
    let after = Asn1Time::from_unix(2_208_988_800)?;
    cert.set_not_before(&before)?;
    cert.set_not_after(&after)?;
    cert.sign(&key, MessageDigest::sha256())?;
    let certificate = cert.build();
    let pem = key.private_key_to_pem_pkcs8()?;
    let identity = Identity::from_pem(&pem)?;
    let bodies = [
        json!({"z": "fim", "push": true, "a": "inicio", "ignore": false, "null": null}),
        json!({"sessionToken": "token", "hashArray": [{"id": 0, "hash": "HASH"}], "algorithm": "SHA256"}),
        json!({"a": "João\n\"", "nested": {"z": i64::MIN, "a": i64::MAX}}),
        json!({"desktopCode": "lab", "pin": "0001", "otp": "001234", "push": false, "nomeAplicacaoDesktop": "Teste", "issue": "issuer", "serialNumber": "serial"}),
        json!({"items": [{"x": "one"}, {"x": "two", "active": true}], "empty": {}, "ignored": []}),
    ];
    // Preserve the old document bytes for compatibility with existing consumers.
    let document = b"DesktopID offline Rust/Python signing interoperability\n";
    let mut signatures = serde_json::Map::new();
    let public = certificate.public_key()?;
    for (algorithm, digest) in [
        (Algorithm::Sha256, MessageDigest::sha256()),
        (Algorithm::Sha384, MessageDigest::sha384()),
        (Algorithm::Sha512, MessageDigest::sha512()),
    ] {
        let mut signer = Signer::new(digest, &key)?;
        signer.set_rsa_padding(Padding::PKCS1)?;
        let signature = signer.sign_oneshot_to_vec(document)?;
        verify_digest(&public, algorithm, &hash(digest, document)?, &signature)?;
        signatures.insert(algorithm.name().into(), json!(STANDARD.encode(signature)));
    }
    let vectors = bodies.iter().map(|body| Ok(json!({"body": body, "flattened_base64": STANDARD.encode(flatten_values(body)?), "authorization": identity.authorization(body)?}))).collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"laboratory_only": true, "generation_note": "Native generator uses SHA256/SHA384/SHA512; SHA1 is excluded by Fedora crypto policy. The historical Python fixture remains unchanged.", "private_key_pem": String::from_utf8(pem)?, "certificate_pem": String::from_utf8(certificate.to_pem()?)?, "document_base64": STANDARD.encode(document), "signatures": signatures, "vectors": vectors}),
    )
}

pub fn generate(output: &Path) -> Result<()> {
    // Default generator formerly replaced a reviewed fixture silently. Require
    // an explicit new destination and refuse both overwrites and canonical path.
    ensure!(
        !output.ends_with(publication::LAB_FIXTURE),
        "O fixture revisado deve ser preservado; escolha uma saída de laboratório em dist/"
    );
    ensure!(
        !output.exists() && fs::symlink_metadata(output).is_err(),
        "A saída já existe; não será substituída"
    );
    let data = serde_json::to_vec_pretty(&vectors()?)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&data)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(output).map_err(|e| e.error)?;
    println!("Public laboratory vectors written: {}", output.display());
    // No checksum exception is updated automatically. New fixtures must be
    // independently reviewed before adding an exact-file publication exception.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::sign::Verifier;

    #[test]
    fn generated_fixture_verifies_and_preserves_original_authorization_vectors() -> Result<()> {
        let fixture = vectors()?;
        let certificate = X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())?;
        let public = certificate.public_key()?;
        let original: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../desktopid-client/tests/fixtures/python-vectors.json"
        ))?;
        for (vector, old) in fixture["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .zip(original["vectors"].as_array().unwrap())
        {
            assert_eq!(vector["body"], old["body"]);
            assert_eq!(vector["flattened_base64"], old["flattened_base64"]);
            let signature = STANDARD.decode(
                vector["authorization"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("Bearer ")
                    .unwrap(),
            )?;
            let mut verifier = Verifier::new(MessageDigest::sha256(), &public)?;
            verifier.set_rsa_padding(Padding::PKCS1)?;
            assert!(verifier.verify_oneshot(
                &signature,
                &STANDARD.decode(vector["flattened_base64"].as_str().unwrap())?
            )?);
        }
        Ok(())
    }

    #[test]
    fn generator_refuses_to_overwrite_a_reviewed_fixture_or_existing_output() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("vectors.json");
        fs::write(&output, "preserve")?;
        assert!(generate(&output).is_err());
        assert_eq!(fs::read(output)?, b"preserve");
        assert!(generate(&directory.path().join(publication::LAB_FIXTURE)).is_err());
        Ok(())
    }
}
