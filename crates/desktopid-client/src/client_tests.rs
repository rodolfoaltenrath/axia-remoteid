use super::*;
use openssl::{
    hash::{MessageDigest, hash},
    pkey::PKey,
    rsa::Padding,
    sign::Verifier,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
    time::Instant,
};

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}
struct Response {
    status: u16,
    body: Value,
    extra_header: Option<String>,
}

fn mock(responses: Vec<Response>) -> (Api, thread::JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    // HTTP loopback is available only in this cfg(test) module, never CLI/config.
    let api = Api::with_base(base).unwrap();
    let thread = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "Mock did not receive expected request"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let method = first.split_whitespace().next().unwrap().to_owned();
            assert!(method == "POST" || method == "GET");
            let path = first.split_whitespace().nth(1).unwrap().to_owned();
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.trim().split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
            let length = headers
                .get("content-length")
                .map(|value| value.parse::<usize>().unwrap())
                .unwrap_or(0);
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            requests.push(Request {
                method,
                path,
                headers,
                body: if bytes.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                },
            });
            let bytes = serde_json::to_vec(&response.body).unwrap();
            let extra = response
                .extra_header
                .map(|v| format!("{v}\r\n"))
                .unwrap_or_default();
            write!(stream, "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n", response.status, bytes.len()).unwrap();
            stream.write_all(&bytes).unwrap();
        }
        requests
    });
    (api, thread)
}

fn response(body: Value) -> Response {
    Response {
        status: 200,
        body,
        extra_header: None,
    }
}

#[test]
fn guided_signing_derives_reference_and_rejects_signature_for_changed_document() {
    use crate::signing::{SigningAuthorization, sign_and_verify};
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/python-vectors.json")).unwrap();
    let entry = WalletCertificate {
        key_name: "SERIAL;ISSUER".into(),
        certificate: X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())
            .unwrap(),
    };
    let document = STANDARD
        .decode(fixture["document_base64"].as_str().unwrap())
        .unwrap();
    let identity = Identity::generate().unwrap();
    for tampered in [false, true] {
        let (api, server) = mock(vec![
            response(json!({"status":true,"token":"lab-token"})),
            response(
                json!({"status":true,"idArray":[{"id":"lab-opaque-id","status":true,
                "signatureBase64":fixture["signatures"]["SHA256"]}]}),
            ),
        ]);
        let input = if tampered {
            b"different document".as_slice()
        } else {
            &document
        };
        let result = sign_and_verify(
            &api,
            &identity,
            "lab-desktop",
            &entry,
            input,
            &SigningAuthorization {
                password: "lab-password",
                otp: "001234",
                push: false,
            },
        );
        assert_eq!(result.is_ok(), !tampered);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/api/signature/tokensessao");
        assert_eq!(
            requests[1].path,
            "/api/signature/requestHashSessionSignature"
        );
        for request in &requests {
            assert_eq!(request.body["serialNumber"], "SERIAL");
            assert_eq!(request.body["issue"], "ISSUER");
        }
        assert_eq!(requests[0].body["otp"], "001234");
        assert_eq!(requests[1].body["algorithm"], "SHA256");
    }
}

#[test]
fn recorded_signature_survives_verification_failure_and_can_be_recovered_offline() {
    use crate::{
        signing::{SigningAuthorization, recover_signing_test, sign_and_verify_recorded},
        storage::{Store, write_new_private},
    };
    use std::os::unix::fs::PermissionsExt;
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/python-vectors.json")).unwrap();
    let entry = WalletCertificate {
        key_name: "SERIAL;ISSUER".into(),
        certificate: X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())
            .unwrap(),
    };
    let document = STANDARD
        .decode(fixture["document_base64"].as_str().unwrap())
        .unwrap();
    let expected = STANDARD
        .decode(fixture["signatures"]["SHA256"].as_str().unwrap())
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (_, identity) = store.load().unwrap();
    let directory = store.new_signing_test_dir().unwrap();
    write_new_private(&directory.join("document.txt"), b"changed document").unwrap();
    write_new_private(
        &directory.join("certificate.pem"),
        &entry.certificate.to_pem().unwrap(),
    )
    .unwrap();
    write_new_private(
        &directory.join("attempt.json"),
        br#"{"operation":"laboratory-signature","algorithm":"SHA256","certificate_number":1}"#,
    )
    .unwrap();
    let (api, server) = mock(vec![
        response(json!({"status":true,"token":"SESSION-SECRET"})),
        response(
            json!({"status":true,"message":"PRIVATE-MESSAGE","idArray":[{
            "id":"PRIVATE-RESULT-ID","status":true,"signatureBase64":fixture["signatures"]["SHA256"]}]}),
        ),
    ]);
    let authorization = SigningAuthorization {
        password: "SECRET-PASSWORD",
        otp: "001234",
        push: false,
    };
    let result = sign_and_verify_recorded(
        &api,
        &identity,
        "lab-desktop",
        &entry,
        b"changed document",
        &authorization,
        Some(&directory),
    );
    assert!(result.is_err());
    assert_eq!(server.join().unwrap().len(), 2);
    let candidate = directory.join("signature-candidate.bin");
    assert_eq!(std::fs::read(&candidate).unwrap(), expected);
    assert_eq!(
        std::fs::metadata(&candidate).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!directory.join("signature.bin").exists());
    let summary =
        std::fs::read_to_string(directory.join("signature-response-summary.json")).unwrap();
    for forbidden in [
        "SESSION-SECRET",
        "PRIVATE-RESULT-ID",
        "PRIVATE-MESSAGE",
        "SECRET-PASSWORD",
        "001234",
    ] {
        assert!(!summary.contains(forbidden));
    }
    let run_id = directory.file_name().unwrap().to_str().unwrap();
    assert!(recover_signing_test(&store, "../identity").is_err());
    assert!(recover_signing_test(&store, run_id).is_err());
    assert!(!directory.join("result.json").exists());
    std::fs::write(directory.join("document.txt"), document).unwrap();
    recover_signing_test(&store, run_id).unwrap();
    let result: Value =
        serde_json::from_slice(&std::fs::read(directory.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["signature_verified"], true);
    assert_eq!(result["recovered_offline"], true);
    assert_eq!(
        std::fs::read(directory.join("signature.bin")).unwrap(),
        expected
    );
    assert!(recover_signing_test(&store, run_id).is_err());
}

#[test]
fn registration_workflow_persists_code_and_does_not_register_twice() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        response(json!({"codigoDesktop":"lab-code"})),
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "lab").unwrap();
    let (mut config, identity) = store.load().unwrap();
    let login = api
        .login("lab@example.invalid", "lab", "lab-password")
        .unwrap();
    let info = DesktopInfo {
        name: "Lab Linux",
        local_user: "test",
        domain: "",
    };
    crate::workflow::register_once(&api, &store, &mut config, &identity, &login, &info).unwrap();
    assert_eq!(
        store.load().unwrap().0.desktop_code.as_deref(),
        Some("lab-code")
    );
    assert!(
        crate::workflow::register_once(&api, &store, &mut config, &identity, &login, &info)
            .is_err()
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].headers["authorization"], "Bearerlab-token");
    assert_eq!(requests[1].body["sistemaOperacional"], "Linux");
}

#[test]
fn failed_registration_preserves_identity_and_blocks_another_remote_attempt() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        Response {
            status: 503,
            body: json!({"private":"not printed"}),
            extra_header: None,
        },
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "lab").unwrap();
    let (mut config, identity) = store.load().unwrap();
    let fingerprint = identity.fingerprint().unwrap();
    let login = api
        .login("lab@example.invalid", "lab", "lab-password")
        .unwrap();
    let info = DesktopInfo {
        name: "Lab Linux",
        local_user: "test",
        domain: "",
    };
    let error = crate::workflow::register_once(&api, &store, &mut config, &identity, &login, &info)
        .unwrap_err();
    assert!(format!("{error:#}").contains("503"));
    assert!(!error.to_string().contains("not printed"));
    assert!(
        crate::workflow::register_once(&api, &store, &mut config, &identity, &login, &info)
            .is_err()
    );
    assert_eq!(store.load().unwrap().1.fingerprint().unwrap(), fingerprint);
    assert!(store.load().unwrap().0.desktop_code.is_none());
    assert!(store.path().join("registration-attempt.json").exists());
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn mock_full_login_registration_wallet_session_signature_roundtrip() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/python-vectors.json")).unwrap();
    let cert = X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes()).unwrap();
    let signature = fixture["signatures"]["SHA256"].as_str().unwrap();
    let (api, server) = mock(vec![
        response(json!({"id": "user", "organizacaoId": 123, "token": "lab-login-token"})),
        response(json!({"codigoDesktop": "lab-desktop"})),
        response(
            json!({"certificados": [{"keyName": "lab-key", "base64": STANDARD.encode(cert.to_der().unwrap())}]}),
        ),
        response(json!({"status": true, "token": "lab-session-token"})),
        response(
            json!({"status": true, "idArray": [{"id":0,"status":true,"signatureBase64":signature}]}),
        ),
    ]);
    let identity = Identity::generate().unwrap();
    let login = api
        .login("lab@example.invalid", "lab-organization", "lab-password")
        .unwrap();
    let desktop = api
        .register(
            &login,
            &DesktopInfo {
                name: "Test Fedora",
                local_user: "test",
                domain: "",
            },
            &identity,
        )
        .unwrap();
    let wallet = api.wallet(&desktop, &identity).unwrap().unwrap();
    assert_eq!(wallet.len(), 1);
    assert_eq!(wallet[0].key_name, "lab-key");
    let reference = CertificateReference {
        issue: "lab-issuer",
        serial: "lab-serial",
    };
    let token = api
        .open_session(&desktop, &reference, "0001", "001234", false, &identity)
        .unwrap();
    let document = STANDARD
        .decode(fixture["document_base64"].as_str().unwrap())
        .unwrap();
    let digest = hash(MessageDigest::sha256(), &document).unwrap();
    let result = api
        .sign_digest(
            &desktop,
            &reference,
            &token,
            Algorithm::Sha256,
            &digest,
            &identity,
        )
        .unwrap();
    let public = wallet[0].certificate.public_key().unwrap();
    protocol::verify_digest(&public, Algorithm::Sha256, &digest, &result).unwrap();
    let requests = server.join().unwrap();
    assert_eq!(
        requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec![
            "/api/manager/usuarios/login/usrsenha",
            "/api/manager/desktopid/usuario/user/organizacao/123",
            "/api/manager/desktopid/lab-desktop/carteira",
            "/api/signature/tokensessao",
            "/api/signature/requestHashSessionSignature"
        ]
    );
    assert!(!requests[0].headers.contains_key("authorization"));
    assert_eq!(
        requests[0].body,
        json!({"email":"lab@example.invalid","urlAcesso":"lab-organization","senha":"lab-password"})
    );
    // The registration header carries no space after "Bearer"; see the
    // evidence cited at the call site in client.rs.
    assert_eq!(
        requests[1].headers["authorization"],
        "Bearerlab-login-token"
    );
    assert_eq!(requests[1].body["sistemaOperacional"], "Linux");
    let public = PKey::public_key_from_pem(
        requests[1].body["chavePublica"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    for request in &requests[2..] {
        let header = &request.headers["authorization"];
        let signature = STANDARD
            .decode(header.strip_prefix("Bearer ").unwrap())
            .unwrap();
        let mut verifier = Verifier::new(MessageDigest::sha256(), &public).unwrap();
        verifier.set_rsa_padding(Padding::PKCS1).unwrap();
        verifier
            .update(&protocol::flatten_values(&request.body).unwrap())
            .unwrap();
        assert!(verifier.verify(&signature).unwrap());
    }
    assert!(
        requests[2].body["momento"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 1700000000
    );
    assert_eq!(requests[3].body["pin"], "0001");
    assert_eq!(requests[3].body["otp"], "001234");
    assert_eq!(
        STANDARD
            .decode(requests[4].body["hashArray"][0]["hash"].as_str().unwrap())
            .unwrap(),
        digest.as_ref()
    );
}

#[test]
fn redirect_does_not_forward_credentials_and_http_errors_omit_server_body() {
    let sensitive = "lab-secret-must-not-be-logged";
    let (api, server) = mock(vec![Response {
        status: 302,
        body: json!({"secret":sensitive}),
        extra_header: Some("Location: https://example.invalid".into()),
    }]);
    let error = api
        .login("lab@example.invalid", "lab", sensitive)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("302"));
    assert!(!error.contains(sensitive));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn wallet_304_and_malformed_login_are_distinct_from_success() {
    let (api, server) = mock(vec![
        Response {
            status: 304,
            body: json!({}),
            extra_header: None,
        },
        response(json!({"id":"user","organizacaoId":"organization"})),
    ]);
    assert!(
        api.wallet("lab", &Identity::generate().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(api.login("lab@example.invalid", "lab", "lab").is_err());
    server.join().unwrap();
}

#[test]
fn endpoints_reject_path_traversal_and_non_https_origins() {
    for url in [
        "http://remoteidcertisign.com.br",
        "https://user:pass@example.invalid",
        "https://example.invalid/api",
        "https://example.invalid?q=1",
    ] {
        assert!(Api::new(url).is_err());
    }
    let api = Api::new("https://example.invalid").unwrap();
    for segment in ["..", ".", "", "bad/part", "bad\\part"] {
        assert!(api.endpoint(&["api", segment]).is_err());
    }
    assert_eq!(
        api.endpoint(&["api", "has space"]).unwrap().path(),
        "/api/has%20space"
    );
}

#[test]
fn registration_bearer_has_no_space_while_signed_requests_keep_it() {
    let (api, server) = mock(vec![
        response(json!({"id": "user", "organizacaoId": 1, "token": "T0KEN"})),
        response(json!({"codigoDesktop": "code"})),
        Response {
            status: 304,
            body: json!({}),
            extra_header: None,
        },
    ]);
    let identity = Identity::generate().unwrap();
    let login = api.login("lab@example.invalid", "", "lab").unwrap();
    api.register(
        &login,
        &DesktopInfo {
            name: "lab",
            local_user: "lab",
            domain: "",
        },
        &identity,
    )
    .unwrap();
    assert!(api.wallet("code", &identity).unwrap().is_none());
    let requests = server.join().unwrap();
    assert_eq!(requests[1].headers["authorization"], "BearerT0KEN");
    assert!(requests[2].headers["authorization"].starts_with("Bearer "));
}

#[test]
fn error_diagnostics_never_print_arbitrary_values_or_echoed_credentials() {
    let secret = "synthetic-secret-password-token";
    let summary = error_summary(
        &serde_json::to_vec(&json!({"Message":secret,
        "ExceptionMessage":format!("Authorization invalid: {secret}"),
        "ExceptionType":"System.NullReferenceException","StackTrace":secret,"email":secret}))
        .unwrap(),
    );
    assert!(!summary.contains(secret));
    assert!(summary.contains("autorizacao"));
    assert!(summary.contains("NullReferenceException"));
    assert!(!error_summary(b"<html>synthetic-secret-password-token</html>").contains(secret));
}

#[test]
fn login_diagnostics_report_structure_without_token_or_user_ids() {
    let login = Login {
        user_id: "private-user-id".into(),
        organization_id: "private-org-id".into(),
        token: Zeroizing::new(" abc.def.ghi ".into()),
    };
    let diagnostics = login.diagnostics();
    assert_eq!(diagnostics["token_has_leading_whitespace"], true);
    assert_eq!(diagnostics["token_has_trailing_whitespace"], true);
    assert_eq!(diagnostics["token_has_three_dot_segments"], true);
    let serialized = diagnostics.to_string();
    for secret in ["private-user-id", "private-org-id", "abc.def.ghi"] {
        assert!(!serialized.contains(secret));
    }
}

#[test]
fn auth_diagnostic_compares_read_only_headers_without_revealing_profile() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        Response {
            status: 500,
            body: json!({"ExceptionType":"System.FormatException","Message":"lab-token"}),
            extra_header: None,
        },
        response(json!({"id":"lab-user","email":"private-profile@example.invalid"})),
    ]);
    let login = api
        .login("lab@example.invalid", "", "lab-password")
        .unwrap();
    let bare = api.authorization_diagnostic(&login, false).unwrap();
    let spaced = api.authorization_diagnostic(&login, true).unwrap();
    assert_eq!(bare["http_status"], 500);
    assert_eq!(spaced["http_status"], 200);
    assert_eq!(spaced["profile_user_id_matches_login"], true);
    let summary = format!("{bare}{spaced}");
    for secret in ["lab-token", "lab-user", "private-profile@example.invalid"] {
        assert!(!summary.contains(secret));
    }
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    for request in &requests[1..] {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.path,
            "/api/manager/usuarios/visualizarproprio/lab-user/lab-org"
        );
        assert!(request.body.is_null());
    }
    assert_eq!(requests[1].headers["authorization"], "Bearerlab-token");
    assert_eq!(requests[2].headers["authorization"], "Bearer lab-token");
}

#[test]
fn explicit_retry_keeps_key_and_history_and_cannot_repeat_again_after_500() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        Response {
            status: 500,
            body: json!({"ExceptionType":"System.NullReferenceException"}),
            extra_header: None,
        },
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, identity) = store.load().unwrap();
    let fingerprint = identity.fingerprint().unwrap();
    assert!(store.ensure_registration_retry_available().is_err());
    store.begin_registration().unwrap();
    let original = std::fs::read(store.path().join("registration-attempt.json")).unwrap();
    let login = api
        .login("lab@example.invalid", "", "lab-password")
        .unwrap();
    let info = DesktopInfo {
        name: "Lab Linux",
        local_user: "lab",
        domain: "",
    };
    let error =
        crate::workflow::register_retry_once(&api, &store, &mut config, &identity, &login, &info)
            .unwrap_err();
    assert!(format!("{error:#}").contains("NullReferenceException"));
    assert_eq!(
        std::fs::read(store.path().join("registration-attempt.before-retry.json")).unwrap(),
        original
    );
    assert_eq!(store.load().unwrap().1.fingerprint().unwrap(), fingerprint);
    assert!(store.ensure_registration_available().is_err());
    assert!(
        crate::workflow::register_retry_once(&api, &store, &mut config, &identity, &login, &info)
            .is_err()
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn explicit_retry_can_persist_a_returned_registration_code() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        response(json!({"codigoDesktop":"lab-recovered-code"})),
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, identity) = store.load().unwrap();
    store.begin_registration().unwrap();
    let login = api
        .login("lab@example.invalid", "", "lab-password")
        .unwrap();
    crate::workflow::register_retry_once(
        &api,
        &store,
        &mut config,
        &identity,
        &login,
        &DesktopInfo {
            name: "Lab Linux",
            local_user: "lab",
            domain: "",
        },
    )
    .unwrap();
    assert_eq!(
        store.load().unwrap().0.desktop_code.as_deref(),
        Some("lab-recovered-code")
    );
    assert!(store.ensure_registration_retry_available().is_err());
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn registration_uses_original_local_fallback_and_preserves_explicit_domain() {
    let (api, server) = mock(vec![
        response(json!({"codigoDesktop":"lab1"})),
        response(json!({"codigoDesktop":"lab2"})),
    ]);
    let identity = Identity::generate().unwrap();
    let login = Login {
        user_id: "lab-user".into(),
        organization_id: "lab-org".into(),
        token: Zeroizing::new("lab-token".into()),
    };
    for domain in ["", "actual-lab-domain"] {
        api.register(
            &login,
            &DesktopInfo {
                name: "lab",
                local_user: "lab",
                domain,
            },
            &identity,
        )
        .unwrap();
    }
    let requests = server.join().unwrap();
    assert_eq!(requests[0].body["dominioRede"], "Local");
    assert_eq!(requests[1].body["dominioRede"], "actual-lab-domain");
    assert_eq!(requests[0].body["sistemaOperacional"], "Linux");
}

#[test]
fn domain_fix_attempt_keeps_identity_and_sends_corrected_field_once() {
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        response(json!({"codigoDesktop":"lab-fixed-code"})),
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, identity) = store.load().unwrap();
    let key_before = std::fs::read(store.path().join("identity.pem")).unwrap();
    store.begin_registration().unwrap();
    store.begin_registration_retry().unwrap();
    let login = api
        .login("lab@example.invalid", "", "lab-password")
        .unwrap();
    crate::workflow::register_domain_fix_once(
        &api,
        &store,
        &mut config,
        &identity,
        &login,
        &DesktopInfo {
            name: "Lab Linux",
            local_user: "lab",
            domain: DEFAULT_NETWORK_DOMAIN,
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read(store.path().join("identity.pem")).unwrap(),
        key_before
    );
    assert!(
        store
            .path()
            .join("registration-attempt.before-domain-fix.json")
            .exists()
    );
    assert_eq!(
        store.load().unwrap().0.desktop_code.as_deref(),
        Some("lab-fixed-code")
    );
    assert!(store.ensure_domain_fix_retry_available().is_err());
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].body["dominioRede"], "Local");
}

#[test]
fn domain_fix_http_error_is_preserved_privately_and_does_not_reopen_retry() {
    use std::os::unix::fs::PermissionsExt;
    let raw =
        json!({"status":500,"error":"Internal Server Error","message":"synthetic secret echo"});
    let (api, server) = mock(vec![
        response(json!({"id":"lab-user","organizacaoId":"lab-org","token":"lab-token"})),
        Response {
            status: 500,
            body: raw.clone(),
            extra_header: None,
        },
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = crate::storage::Store::new(temporary.path().join("identity")).unwrap();
    store.initialize("https://example.invalid", "").unwrap();
    let (mut config, identity) = store.load().unwrap();
    store.begin_registration().unwrap();
    store.begin_registration_retry().unwrap();
    let login = api
        .login("lab@example.invalid", "", "lab-password")
        .unwrap();
    let error = crate::workflow::register_domain_fix_once(
        &api,
        &store,
        &mut config,
        &identity,
        &login,
        &DesktopInfo {
            name: "Lab Linux",
            local_user: "lab",
            domain: DEFAULT_NETWORK_DOMAIN,
        },
    )
    .unwrap_err();
    assert!(!format!("{error:#}").contains("synthetic secret echo"));
    let path = std::fs::read_dir(store.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("registration-error-")
        })
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let body: Value = serde_json::from_slice(
        &STANDARD
            .decode(saved["body_base64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body, raw);
    assert!(store.ensure_domain_fix_retry_available().is_err());
    assert!(store.ensure_registration_available().is_err());
    assert_eq!(server.join().unwrap().len(), 2);
}
