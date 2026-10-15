//! Graphical connection using the same one-time registration workflow as the CLI.
use crate::{
    client::{Api, DEFAULT_NETWORK_DOMAIN, DesktopInfo},
    protocol::reference_from_key_name,
    storage::Store,
    workflow::register_once,
};
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
};
use zeroize::Zeroizing;

const DEFAULT_SERVER: &str = "https://remoteidcertisign.com.br";
const DEFAULT_DESKTOP_NAME: &str = "Axia RemotedID Fedora Linux";

pub(crate) struct Credentials {
    pub email: String,
    pub password: Zeroizing<String>,
    pub desktop_name: String,
}

pub(crate) enum Action {
    Register(Credentials),
    Refresh,
    Cancel,
}

pub(crate) struct ConnectionSummary {
    pub registered_now: bool,
    pub wallet_updated: bool,
    pub certificate_count: usize,
    pub compatible_count: usize,
}

/// Inject dialogs for synthetic tests; only an explicit Register action can
/// log in and register. An existing installation never requests credentials.
pub(crate) fn connect_with(
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

fn zenity() -> Command {
    let mut command = Command::new("/usr/bin/zenity");
    command
        .arg("--title=Axia RemotedID — Certificado em nuvem")
        .arg("--width=520")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn parse_credentials(raw: &[u8]) -> Result<Credentials> {
    let text = std::str::from_utf8(raw).context("Entrada da janela inválida")?;
    let mut fields = text.strip_suffix('\n').unwrap_or(text).split('\n');
    let email = fields.next().context("E-mail ausente")?.trim().to_owned();
    let password = Zeroizing::new(fields.next().context("Senha ausente")?.to_owned());
    let desktop_name = fields
        .next()
        .context("Nome do computador ausente")?
        .to_owned();
    ensure!(fields.next().is_none(), "Entrada da janela inválida");
    Ok(Credentials {
        email,
        password,
        desktop_name,
    })
}

fn prompt_action(registered: bool) -> Result<Action> {
    let mut dialog = zenity();
    if registered {
        dialog.args([
            "--question",
            "--text=Este computador já está cadastrado.\nAtualizar os certificados disponíveis?",
            "--ok-label=Atualizar certificados",
            "--cancel-label=Fechar",
        ]);
    } else {
        dialog.args([
            "--forms",
            "--text=Entre com os dados do aplicativo Certisign.\nAo conectar, este computador será cadastrado na sua conta.\nO nome do computador é opcional.",
            "--add-entry=E-mail",
            "--add-password=Senha do aplicativo Certisign",
            "--add-entry=Nome deste computador",
            "--separator=\n",
            "--ok-label=Conectar e cadastrar",
            "--cancel-label=Cancelar",
        ]);
    }
    let output = dialog
        .output()
        .context("Não foi possível abrir a janela de conexão")?;
    let raw = Zeroizing::new(output.stdout);
    match output.status.code() {
        Some(1) => Ok(Action::Cancel),
        Some(0) if registered => Ok(Action::Refresh),
        Some(0) => Ok(Action::Register(parse_credentials(&raw)?)),
        _ => anyhow::bail!("Não foi possível exibir a janela de conexão"),
    }
}

fn prompt_autostart() -> Result<bool> {
    let status = zenity().args([
        "--question",
        "--text=Deseja iniciar o Axia RemotedID com o Fedora?\nEle ficará na bandeja, sem abrir uma janela.\nVocê pode alterar essa opção depois.",
        "--ok-label=Ativar",
        "--cancel-label=Agora não",
    ]).status().context("Não foi possível exibir a opção de inicialização")?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => anyhow::bail!("Não foi possível exibir a opção de inicialização"),
    }
}

#[derive(Default)]
struct Progress(Option<Child>);

impl Progress {
    fn update(&mut self, text: &str) -> Result<()> {
        if self.0.is_none() {
            self.0 = Some(
                zenity()
                    .args(["--progress", "--pulsate", "--no-cancel", "--auto-close"])
                    .arg(format!("--text={text}"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .context("Não foi possível abrir a janela de progresso")?,
            );
        }
        self.0
            .as_mut()
            .and_then(|child| child.stdin.as_mut())
            .context("Janela de progresso indisponível")?
            .write_all(format!("#{text}\n").as_bytes())?;
        Ok(())
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn message(kind: &str, text: &str) -> Result<()> {
    let status = zenity()
        .arg(kind)
        .arg("--no-markup")
        .arg(format!("--text={text}"))
        .status()
        .context("Não foi possível exibir a mensagem")?;
    ensure!(
        matches!(status.code(), Some(0 | 1)),
        "Falha na janela de mensagem"
    );
    Ok(())
}

fn error_message(error: &anyhow::Error) -> String {
    if let Some(service) = error.downcast_ref::<crate::client::ServiceError>() {
        format!("{}\n\nO serviço respondeu HTTP {}.", error, service.status)
    } else {
        error.to_string()
    }
}

pub fn run(store: &Store) -> Result<()> {
    ensure!(
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        "Sessão gráfica indisponível. Execute em uma sessão do GNOME ou use connect no terminal"
    );
    let mut progress = Progress::default();
    let result = (|| {
        crate::autostart::first_run(
            &crate::autostart::config_root()?,
            &crate::autostart::executable()?,
            store.path(),
            prompt_autostart,
        )?;
        // Start the windowless resident after recording the first-run choice.
        // Closing the GUI leaves it running; choosing Exit in its menu must
        // not launch it again when this GUI subsequently closes.
        if let Err(error) = crate::tray::start_background(store) {
            eprintln!("Não foi possível iniciar a bandeja: {error}");
        }
        let server = if store.path().try_exists()? {
            store.load()?.0.server
        } else {
            DEFAULT_SERVER.to_owned()
        };
        let api = Api::new(&server)?;
        connect_with(&api, store, prompt_action, |text| progress.update(text))
    })();
    drop(progress);
    match result {
        Ok(None) => Ok(()),
        Ok(Some(summary)) => message(
            "--info",
            &format!(
                "{}\n\n{}\nCertificados disponíveis: {}\nCom referência de assinatura reconhecida: {}\n\nNenhuma assinatura foi solicitada.",
                if summary.registered_now {
                    "Login aceito e computador cadastrado."
                } else {
                    "Cadastro existente reutilizado."
                },
                if summary.wallet_updated {
                    "Certificados atualizados e salvos neste computador."
                } else {
                    "O serviço informou que não há alteração na carteira."
                },
                summary.certificate_count,
                summary.compatible_count,
            ),
        ),
        Err(error) => {
            let _ = message("--error", &error_message(&error));
            Err(error)
        }
    }
}

/// Session startup is local only. Network/account actions require a UI click.
pub fn run_startup(store: &Store) -> Result<()> {
    if !crate::autostart::enabled(&crate::autostart::config_root()?)? {
        return Ok(());
    }
    crate::tray::run(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_preserve_password_whitespace_and_reject_extra_fields_without_echoing() {
        let credentials =
            parse_credentials(b" lab@example.invalid \n secret | value \n\n").unwrap();
        assert_eq!(credentials.email, "lab@example.invalid");
        assert_eq!(credentials.password.as_str(), " secret | value ");
        assert!(credentials.desktop_name.is_empty());
        let error = parse_credentials(b"lab@example.invalid\nsecret\nFedora\nextra\n")
            .err()
            .unwrap();
        assert!(!error.to_string().contains("secret"));
    }
}
