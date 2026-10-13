use base64::{Engine, engine::general_purpose::STANDARD};
use desktopid_client::{
    identity::Identity,
    protocol::{
        self, Algorithm, CertificateReference, SESSION_CACHE_LIFETIME, reference_from_key_name,
    },
    storage::Store,
};
use openssl::{hash::hash, x509::X509};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/python-vectors.json")).unwrap()
}

#[test]
fn public_wallet_inventory_does_not_read_installation_private_key() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, _) = store.load().unwrap();
    config.desktop_code = Some("lab-code".into());
    store.save_registration(&config).unwrap();
    let data = fixture();
    store
        .save_wallet(&[desktopid_client::client::WalletCertificate {
            key_name: "SERIAL;ISSUER".into(),
            certificate: X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes())
                .unwrap(),
        }])
        .unwrap();
    fs::rename(
        store.path().join("identity.pem"),
        store.path().join("private-key-not-available"),
    )
    .unwrap();
    assert_eq!(store.load_wallet_public().unwrap().len(), 1);
    assert!(store.load_wallet().is_err());
    let public = store.path().join("public.pem");
    fs::write(&public, Identity::generate().unwrap().public_pem().unwrap()).unwrap();
    assert!(store.load_wallet_public().is_err());
}

#[test]
fn cached_wallet_is_bound_to_installation_and_rejects_tampering() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, _) = store.load().unwrap();
    config.desktop_code = Some("lab-desktop".into());
    store.save_registration(&config).unwrap();
    let data = fixture();
    let cert = X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap();
    store
        .save_wallet(&[desktopid_client::client::WalletCertificate {
            key_name: "SERIAL;ISSUER".into(),
            certificate: cert.clone(),
        }])
        .unwrap();
    let restored = store.load_wallet().unwrap();
    assert_eq!(restored[0].key_name, "SERIAL;ISSUER");
    assert_eq!(
        restored[0].certificate.to_der().unwrap(),
        cert.to_der().unwrap()
    );
    let path = store.path().join("wallet.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for field in ["public_key_sha256", "desktop_code", "schema_version"] {
        let mut changed = original.clone();
        changed[field] = json!("different");
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(store.load_wallet().is_err());
    }
    let mut changed = original.clone();
    changed["certificados"][0]["index"] = json!(1);
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(store.load_wallet().is_err());
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.load_wallet().is_err());
}

#[test]
fn signing_preflight_rejects_expired_future_and_bad_reference() {
    let data = fixture();
    let mut entry = desktopid_client::client::WalletCertificate {
        key_name: "SERIAL;ISSUER".into(),
        certificate: X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap(),
    };
    use desktopid_client::signing::validate_certificate;
    validate_certificate(&entry, 1791936000).unwrap();
    assert!(validate_certificate(&entry, 0).is_err());
    assert!(validate_certificate(&entry, 2524608000).is_err());
    entry.key_name = "unrecognized".into();
    assert!(validate_certificate(&entry, 1791936000).is_err());
}

#[test]
fn laboratory_runs_keep_separate_private_artifacts_and_preserve_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let before = fs::read(store.path().join("identity.pem")).unwrap();
    let first = store.new_signing_test_dir().unwrap();
    let second = store.new_signing_test_dir().unwrap();
    assert_ne!(first, second);
    assert_eq!(
        fs::metadata(&first).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let doc = first.join("document.txt");
    desktopid_client::storage::write_new_private(&doc, desktopid_client::signing::LAB_DOCUMENT)
        .unwrap();
    assert_eq!(
        fs::metadata(&doc).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(desktopid_client::storage::write_new_private(&doc, b"overwrite").is_err());
    assert_eq!(fs::read(store.path().join("identity.pem")).unwrap(), before);
}

#[test]
fn python_generated_authentication_vectors_match_byte_for_byte() {
    let data = fixture();
    assert_eq!(data["laboratory_only"], true);
    let identity =
        Identity::from_pem(data["private_key_pem"].as_str().unwrap().as_bytes()).unwrap();
    for vector in data["vectors"].as_array().unwrap() {
        assert_eq!(
            protocol::flatten_values(&vector["body"]).unwrap(),
            STANDARD
                .decode(vector["flattened_base64"].as_str().unwrap())
                .unwrap()
        );
        assert_eq!(
            identity.authorization(&vector["body"]).unwrap(),
            vector["authorization"].as_str().unwrap()
        );
    }
}

#[test]
fn unsupported_json_cannot_be_signed() {
    for input in [
        json!({"n": 1.25}),
        json!({"n": 9223372036854775808u64}),
        json!({"a": [1, 2]}),
        json!([{}]),
        json!({"a": [null]}),
    ] {
        assert!(protocol::flatten_values(&input).is_err());
    }
}

#[test]
fn python_prehashed_signatures_verify_and_tampered_or_double_hash_fail() {
    let data = fixture();
    let cert = X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap();
    let key = cert.public_key().unwrap();
    let document = STANDARD
        .decode(data["document_base64"].as_str().unwrap())
        .unwrap();
    for algorithm in [Algorithm::Sha256, Algorithm::Sha384, Algorithm::Sha512] {
        let digest = hash(algorithm.message_digest(), &document).unwrap();
        let signature = STANDARD
            .decode(data["signatures"][algorithm.name()].as_str().unwrap())
            .unwrap();
        protocol::verify_digest(&key, algorithm, &digest, &signature).unwrap();
        let doubled = hash(algorithm.message_digest(), &digest).unwrap();
        assert!(protocol::verify_digest(&key, algorithm, &doubled, &signature).is_err());
        let mut corrupt = signature;
        corrupt[0] ^= 1;
        assert!(protocol::verify_digest(&key, algorithm, &digest, &corrupt).is_err());
    }
}

#[test]
fn request_keeps_leading_zeroes_and_sends_digest_only() {
    let reference = CertificateReference {
        issue: "issuer",
        serial: "serial",
    };
    let body = protocol::session_payload("lab", &reference, "0001", "001234", false, "test");
    assert_eq!(body["pin"], "0001");
    assert_eq!(body["otp"], "001234");
    assert_eq!(body["push"], false);
    let body = protocol::signature_payload("lab", &reference, "token", Algorithm::Sha256, &[7; 32])
        .unwrap();
    assert_eq!(
        STANDARD
            .decode(body["hashArray"][0]["hash"].as_str().unwrap())
            .unwrap(),
        [7; 32]
    );
    assert!(
        protocol::signature_payload("lab", &reference, "token", Algorithm::Sha256, &[7; 31])
            .is_err()
    );
}

#[test]
fn signature_response_requires_one_successful_well_formed_result() {
    for response in [
        json!({"status": false}),
        json!({"status": "true", "idArray": []}),
        json!({"status": true, "idArray": []}),
        json!({"status": true,"idArray": [{"status":true,"signatureBase64":"%%%"}]}),
        json!({"status":true,"idArray":[{"status":false,"signatureBase64":"YQ=="}]}),
        json!({"status":true,"idArray":[{"status":true,"signatureBase64":"YQ=="},{"status":true,"signatureBase64":"YQ=="}]}),
        json!({"status":true,"idArray":[{"status":true,"signatureBase64":""}]}),
    ] {
        assert!(protocol::decode_signature(&response).is_err());
    }
    assert_eq!(
        protocol::decode_signature(
            &json!({"status":true,"idArray":[{"status":true,"id":0,"signatureBase64":"YQ=="}]})
        )
        .unwrap(),
        b"a"
    );
}

#[test]
fn response_id_is_metadata_and_does_not_have_to_echo_numeric_zero() {
    for id in [
        json!(0),
        json!(9),
        json!("0"),
        json!("lab-opaque-id"),
        Value::Null,
    ] {
        let response =
            json!({"status":true,"idArray":[{"status":true,"id":id,"signatureBase64":"YQ=="}]});
        assert_eq!(protocol::decode_signature(&response).unwrap(), b"a");
    }
    assert_eq!(
        protocol::decode_signature(
            &json!({"status":true,"idArray":[{"status":true,"signatureBase64":"YQ=="}]})
        )
        .unwrap(),
        b"a"
    );
}

#[test]
fn response_summary_omits_identifiers_credentials_and_signature_bytes() {
    let response = json!({"status":true,"token":"SECRET-TOKEN","message":"SECRET-MESSAGE",
        "idArray":[{"status":true,"id":"PRIVATE-ID","signatureBase64":"PRIVATE-SIGNATURE","message":"PRIVATE-TEXT"}]});
    let summary = protocol::signature_response_summary(&response);
    assert_eq!(summary["id_type"], "string");
    assert_eq!(summary["id_is_numeric_zero"], false);
    assert_eq!(summary["result_count"], 1);
    assert_eq!(summary["service_status"], true);
    let text = summary.to_string();
    for secret in ["SECRET", "PRIVATE", "TOKEN", "MESSAGE", "SIGNATURE"] {
        assert!(!text.contains(secret));
    }
}

#[test]
fn identity_is_persistent_private_and_never_replaced() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("identity");
    let store = Store::new(root.clone()).unwrap();
    let first = store
        .initialize("https://example.invalid", "laboratory")
        .unwrap();
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.join("identity.pem"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let (_, identity) = store.load().unwrap();
    assert_eq!(first.public_key_sha256, identity.fingerprint().unwrap());
    assert!(
        store
            .initialize("https://example.invalid", "laboratory")
            .is_err()
    );
    fs::set_permissions(root.join("identity.pem"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.load().is_err());
}

#[test]
fn identity_rejects_symlinks_and_key_configuration_mismatch() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("identity");
    let store = Store::new(root.clone()).unwrap();
    store
        .initialize("https://example.invalid", "laboratory")
        .unwrap();
    let other = Identity::generate().unwrap();
    fs::write(root.join("identity.pem"), &*other.private_pem().unwrap()).unwrap();
    assert!(store.load().is_err());
    fs::rename(root.join("identity.pem"), root.join("other.pem")).unwrap();
    std::os::unix::fs::symlink(root.join("other.pem"), root.join("identity.pem")).unwrap();
    assert!(store.load().is_err());
    assert!(
        Store::new(std::path::PathBuf::from(
            "/mnt/c/Users/Altenrath/test-identity"
        ))
        .is_err()
    );
}

#[test]
fn registration_journal_preserves_the_returned_code() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    let mut config = store
        .initialize("https://example.invalid", "laboratory")
        .unwrap();
    config.desktop_code = Some("lab-code".into());
    store.save_registration(&config).unwrap();
    assert_eq!(
        store.load().unwrap().0.desktop_code.as_deref(),
        Some("lab-code")
    );
    let journal: Value =
        serde_json::from_slice(&fs::read(store.path().join("registration.json")).unwrap()).unwrap();
    assert_eq!(journal["desktop_code"], "lab-code");
    assert!(store.save_registration(&config).is_err());
}

#[test]
fn wallet_cache_keeps_original_key_name_and_updates_without_exposing_secrets() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    let mut config = store.initialize("https://example.invalid", "lab").unwrap();
    assert!(store.save_wallet(&[]).is_err());
    config.desktop_code = Some("lab-code".into());
    store.save_registration(&config).unwrap();
    let data = fixture();
    let cert = X509::from_pem(data["certificate_pem"].as_str().unwrap().as_bytes()).unwrap();
    let entry = desktopid_client::client::WalletCertificate {
        key_name: "SERIE;ISSUER".into(),
        certificate: cert.clone(),
    };
    store.save_wallet(&[entry]).unwrap();
    let path = store.path().join("wallet.json");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let bytes = fs::read(&path).unwrap();
    let wallet: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wallet["certificados"][0]["keyName"], "SERIE;ISSUER");
    assert_eq!(wallet["public_key_sha256"], config.public_key_sha256);
    assert_eq!(
        STANDARD
            .decode(wallet["certificados"][0]["base64"].as_str().unwrap())
            .unwrap(),
        cert.to_der().unwrap()
    );
    assert!(!String::from_utf8(bytes).unwrap().contains("PRIVATE KEY"));
    store.save_wallet(&[]).unwrap();
    let wallet: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(wallet["certificados"].as_array().unwrap().len(), 0);
}

#[test]
fn key_name_splits_into_serial_then_issue() {
    // openSession @0x60ed2 / signHash @0x61c50: parts[0] -> serialNumber,
    // parts[1] -> issue. Serial comes first; this is not the X.509 display order.
    let (serial, issue) = reference_from_key_name("0A1B2C3D;AC Certisign RFB G5").unwrap();
    assert_eq!(serial, "0A1B2C3D");
    assert_eq!(issue, "AC Certisign RFB G5");
}

#[test]
fn key_name_compresses_separator_runs_like_boost_token_compress_on() {
    let (serial, issue) = reference_from_key_name("SERIE;;;EMISSOR").unwrap();
    assert_eq!((serial.as_str(), issue.as_str()), ("SERIE", "EMISSOR"));
}

#[test]
fn key_name_rejects_missing_parts_and_non_ascii() {
    for bad in ["semseparador", "a;b;c", "", ";", "SERIE;"] {
        assert!(
            reference_from_key_name(bad).is_err(),
            "deveria recusar {bad:?}"
        );
    }
    // The original narrows wchar_t to char when splitting, so non-ASCII has no
    // confirmed representation and must not be guessed.
    assert!(reference_from_key_name("SERIE;EMISSÃO").is_err());
}

#[test]
fn session_cache_lifetime_matches_recovered_token_store() {
    assert_eq!(SESSION_CACHE_LIFETIME.as_secs(), 1800);
}
