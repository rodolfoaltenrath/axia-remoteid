use super::core::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use cryptoki_sys::*;
use desktopid_client::{
    client::WalletCertificate,
    protocol::{Algorithm, verify_digest},
};
use openssl::{hash::hash, x509::X509};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../desktopid-client/tests/fixtures/python-vectors.json"
    ))
    .unwrap()
}
#[test]
fn hashing_mechanisms_match_python_signatures_and_reject_modified_data() {
    let data = fixture();
    let document = STANDARD
        .decode(data["document_base64"].as_str().unwrap())
        .unwrap();
    let entry = WalletCertificate {
        key_name: "SERIAL;ISSUER".into(),
        certificate: X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap(),
    };
    for (mechanism, label) in [
        (CKM_SHA256_RSA_PKCS, "SHA256"),
        (CKM_SHA384_RSA_PKCS, "SHA384"),
        (CKM_SHA512_RSA_PKCS, "SHA512"),
    ] {
        let (algorithm, digest) = input_digest(mechanism, &document).unwrap();
        let signature = STANDARD
            .decode(data["signatures"][label].as_str().unwrap())
            .unwrap();
        check_signature(&entry, algorithm, &digest, &signature).unwrap();
        let (_, changed) = input_digest(mechanism, b"modified document").unwrap();
        assert!(check_signature(&entry, algorithm, &changed, &signature).is_err());
    }
}
#[test]
fn rsa_pkcs_digest_info_is_not_hashed_twice_and_wrong_encodings_are_rejected() {
    let data = fixture();
    let document = STANDARD
        .decode(data["document_base64"].as_str().unwrap())
        .unwrap();
    let digest = hash(Algorithm::Sha256.message_digest(), &document).unwrap();
    let mut encoded = vec![
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    encoded.extend_from_slice(&digest);
    let (algorithm, decoded) = input_digest(CKM_RSA_PKCS, &encoded).unwrap();
    assert_eq!(decoded, digest.as_ref());
    let cert = X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap();
    let signature = STANDARD
        .decode(data["signatures"]["SHA256"].as_str().unwrap())
        .unwrap();
    verify_digest(&cert.public_key().unwrap(), algorithm, &decoded, &signature).unwrap();
    encoded.push(0);
    assert_eq!(
        input_digest(CKM_RSA_PKCS, &encoded).err(),
        Some(CKR_DATA_INVALID)
    );
    assert_eq!(
        input_digest(CKM_RSA_PKCS, &digest).err(),
        Some(CKR_DATA_INVALID)
    );
    assert_eq!(
        input_digest(CKM_RSA_PKCS_PSS, &digest).err(),
        Some(CKR_MECHANISM_INVALID)
    );
}
