use super::*;
use std::fs;

fn registered_store(root: &std::path::Path) -> Store {
    let store = Store::new(root.join("identity")).unwrap();
    let mut config = store.initialize("https://example.invalid", "").unwrap();
    config.desktop_code = Some("lab-desktop".into());
    store.save_registration(&config).unwrap();
    store
}

fn save_wallet(store: &Store) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../desktopid-client/tests/fixtures/python-vectors.json"
    ))
    .unwrap();
    store
        .save_wallet(&[crate::client::WalletCertificate {
            key_name: "LAB-SERIAL;LAB-ISSUER".into(),
            certificate: openssl::x509::X509::from_pem(
                fixture["certificate_pem"].as_str().unwrap().as_bytes(),
            )
            .unwrap(),
        }])
        .unwrap();
}

#[test]
fn saved_registration_without_wallet_offers_refresh_without_repeating_preflight() {
    let temporary = tempfile::tempdir().unwrap();
    let store = registered_store(temporary.path());
    // This guard deliberately rejects the old UI's repeated registration path.
    assert!(store.ensure_registration_available().is_err());
    let identity = fs::read(store.path().join("identity.pem")).unwrap();
    let journal = fs::read(store.path().join("registration.json")).unwrap();
    let state = ConnectionState::read(&store).unwrap();
    assert!(state.registered);
    assert!(state.certificate_count.is_none());
    assert_eq!(state.connect_label(), "Atualizar certificados");
    assert!(state.status().contains("Computador cadastrado"));
    assert!(matches!(
        connection_action(&store, prompt_action).unwrap(),
        Action::Refresh
    ));
    assert_eq!(
        fs::read(store.path().join("identity.pem")).unwrap(),
        identity
    );
    assert_eq!(
        fs::read(store.path().join("registration.json")).unwrap(),
        journal
    );
    assert!(!store.path().join("wallet.json").exists());
}

#[test]
fn wallet_availability_is_validated_again_for_every_connection_state_read() {
    let temporary = tempfile::tempdir().unwrap();
    let store = registered_store(temporary.path());
    save_wallet(&store);
    assert_eq!(
        ConnectionState::read(&store).unwrap().certificate_count,
        Some(1)
    );
    fs::write(
        store.path().join("wallet.json"),
        b"invalid laboratory cache",
    )
    .unwrap();
    let state = ConnectionState::read(&store).unwrap();
    assert!(state.registered);
    assert!(state.certificate_count.is_none());
    assert!(matches!(
        connection_action(&store, prompt_action).unwrap(),
        Action::Refresh
    ));
    save_wallet(&store);
    assert_eq!(
        ConnectionState::read(&store).unwrap().certificate_count,
        Some(1)
    );
}

#[test]
fn cancelled_connection_does_not_create_identity_and_ambiguous_attempt_stays_blocked() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    assert!(matches!(
        connection_action(&store, |registered| {
            assert!(!registered);
            Ok(Action::Cancel)
        })
        .unwrap(),
        Action::Cancel
    ));
    assert!(!store.path().exists());
    store.initialize("https://example.invalid", "").unwrap();
    store.begin_registration().unwrap();
    assert!(
        connection_action(&store, |_| panic!(
            "ambiguous attempt must block credentials"
        ))
        .is_err()
    );
    assert!(!ConnectionState::read(&store).unwrap().registered);
}

#[test]
fn unreadable_identity_never_prompts_for_another_registration() {
    let temporary = tempfile::tempdir().unwrap();
    let store = registered_store(temporary.path());
    fs::write(
        store.path().join("config.json"),
        b"invalid laboratory config",
    )
    .unwrap();
    assert!(ConnectionState::read(&store).is_err());
    assert!(
        connection_action(&store, |_| panic!(
            "unreadable state must block credentials"
        ))
        .is_err()
    );
}

/// Called by the existing Broadway test on its single GTK thread. Never run a
/// second GTK-initializing Rust test on a different libtest worker thread.
pub(crate) fn exercise_connection_recovery() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("identity")).unwrap();
    let connect = gtk::Button::with_label("Conectar conta");
    let browsers = gtk::Button::with_label("Configurar navegadores");
    let status = gtk::Label::new(None);
    browsers.set_sensitive(false);
    complete_connection(&store, Ok(None), &connect, &browsers, &status);
    assert_eq!(connect.label().as_deref(), Some("Conectar conta"));
    assert!(!store.path().exists());
    assert!(!browsers.is_sensitive());

    let worker_store = store.path().to_owned();
    let result = crate::ui::work(
        "Cadastro sintético; consulta da carteira falha",
        move |_| {
            let store = Store::new(worker_store)?;
            let mut config = store.initialize("https://example.invalid", "")?;
            store.begin_registration()?;
            config.desktop_code = Some("lab-desktop".into());
            store.save_registration(&config)?;
            anyhow::bail!("O cadastro está salvo; não foi possível consultar os certificados")
        },
    );
    assert!(result.is_err());
    complete_connection(&store, result, &connect, &browsers, &status);
    assert_eq!(connect.label().as_deref(), Some("Atualizar certificados"));
    assert!(connect.is_sensitive());
    assert!(!browsers.is_sensitive());
    assert!(status.text().contains("sem novo cadastro"));
    let identity = fs::read(store.path().join("identity.pem")).unwrap();
    let journal = fs::read(store.path().join("registration.json")).unwrap();

    // The second action uses the same controls and saved identity; no dialog
    // or fresh registration is allowed, even after another wallet failure.
    assert!(matches!(
        connection_action(&store, prompt_action).unwrap(),
        Action::Refresh
    ));
    complete_connection(
        &store,
        Err(anyhow::anyhow!("Consulta indisponível")),
        &connect,
        &browsers,
        &status,
    );
    assert_eq!(connect.label().as_deref(), Some("Atualizar certificados"));
    assert!(!browsers.is_sensitive());
    save_wallet(&store);
    complete_connection(
        &store,
        Ok(Some(ConnectionSummary {
            registered_now: false,
            wallet_updated: true,
            certificate_count: 1,
            compatible_count: 1,
        })),
        &connect,
        &browsers,
        &status,
    );
    assert!(browsers.is_sensitive());
    assert!(status.text().contains("Certificados disponíveis: 1"));
    complete_connection(
        &store,
        Err(anyhow::anyhow!("Consulta indisponível")),
        &connect,
        &browsers,
        &status,
    );
    assert!(browsers.is_sensitive());
    assert_eq!(
        fs::read(store.path().join("identity.pem")).unwrap(),
        identity
    );
    assert_eq!(
        fs::read(store.path().join("registration.json")).unwrap(),
        journal
    );

    fs::write(
        store.path().join("config.json"),
        b"invalid laboratory config",
    )
    .unwrap();
    complete_connection(
        &store,
        Err(anyhow::anyhow!("Falha sintética")),
        &connect,
        &browsers,
        &status,
    );
    assert!(!connect.is_sensitive());
    assert!(!browsers.is_sensitive());
    assert!(status.text().contains("estado local"));
}
