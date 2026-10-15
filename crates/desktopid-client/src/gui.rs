//! Graphical connection using the same one-time registration workflow as the CLI.
use crate::{
    client::{Api, DEFAULT_NETWORK_DOMAIN, DesktopInfo},
    protocol::reference_from_key_name,
    storage::Store,
    workflow::register_once,
};
use anyhow::{Context, Result, ensure};
use zeroize::Zeroizing;

const DEFAULT_SERVER: &str = "https://remoteidcertisign.com.br";
const DEFAULT_DESKTOP_NAME: &str = "Axia RemotedID Fedora Linux";

pub struct Credentials {
    pub email: String,
    pub password: Zeroizing<String>,
    pub desktop_name: String,
}

pub enum Action {
    Register(Credentials),
    Refresh,
    Cancel,
}

pub struct ConnectionSummary {
    pub registered_now: bool,
    pub wallet_updated: bool,
    pub certificate_count: usize,
    pub compatible_count: usize,
}

/// Inject dialogs for synthetic tests; only an explicit Register action can
/// log in and register. An existing installation never requests credentials.
pub fn connect_with(
    api: &Api,
    store: &Store,
    action: impl FnOnce(bool) -> Result<Action>,
    mut progress: impl FnMut(&str) -> Result<()>,
) -> Result<Option<ConnectionSummary>> {
    let mut saved = if store.path().try_exists()? {
        Some(store.load()?)
    } else {
        None
    };
    let registered = saved
        .as_ref()
        .is_some_and(|(config, _)| config.desktop_code.is_some());
    if saved.is_some() && !registered {
        store.ensure_registration_available()?;
    }
    match action(registered)? {
        Action::Cancel => return Ok(None),
        Action::Refresh => ensure!(registered, "Esta instalação ainda não foi cadastrada"),
        Action::Register(credentials) => {
            ensure!(!registered, "Esta instalação já possui cadastro");
            ensure!(
                !credentials.email.trim().is_empty() && credentials.email.contains('@'),
                "Informe um e-mail válido"
            );
            ensure!(
                !credentials.password.is_empty(),
                "Informe a senha do aplicativo"
            );
            progress("Verificando o login...")?;
            let access_url = saved
                .as_ref()
                .map(|(config, _)| config.access_url.as_str())
                .unwrap_or("");
            let login = api
                .login(&credentials.email, access_url, &credentials.password)
                .context(
                    "Não foi possível fazer login. O cadastro deste computador não foi iniciado",
                )?;
            if saved.is_none() {
                store.initialize(DEFAULT_SERVER, "")?;
                saved = Some(store.load()?);
            }
            let (config, identity) = saved.as_mut().context("Identidade local ausente")?;
            progress("Login aceito. Cadastrando este computador...")?;
            let user = std::env::var("USER").unwrap_or_else(|_| "linux".into());
            let desktop_name = if credentials.desktop_name.trim().is_empty() {
                DEFAULT_DESKTOP_NAME
            } else {
                credentials.desktop_name.trim()
            };
            register_once(
                api,
                store,
                config,
                identity,
                &login,
                &DesktopInfo {
                    name: desktop_name,
                    local_user: &user,
                    domain: DEFAULT_NETWORK_DOMAIN,
                },
            )
            .context("Não foi possível confirmar o cadastro. A identidade e o registro da tentativa foram preservados")?;
        }
    }
    let (config, identity) = saved.context("Identidade local ausente")?;
    let desktop = config.desktop_code.as_deref().context("Cadastro ausente")?;
    progress("Instalação cadastrada. Consultando os certificados...")?;
    let response = api
        .wallet(desktop, &identity)
        .context("O cadastro está salvo; não foi possível consultar os certificados")?;
    let wallet_updated = response.is_some();
    let wallet = match response {
        Some(wallet) => {
            store.save_wallet(&wallet)?;
            wallet
        }
        None if store.path().join("wallet.json").try_exists()? => store.load_wallet()?,
        None => Vec::new(),
    };
    Ok(Some(ConnectionSummary {
        registered_now: !registered,
        wallet_updated,
        certificate_count: wallet.len(),
        compatible_count: wallet
            .iter()
            .filter(|entry| reference_from_key_name(&entry.key_name).is_ok())
            .count(),
    }))
}
