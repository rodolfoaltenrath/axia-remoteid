use crate::{client::Api, storage::Store};
use anyhow::{Context, Result};
use desktopid_client::gui::{Action, ConnectionSummary, Credentials, connect_with};
use gtk::{glib, prelude::*};
use std::{cell::Cell, rc::Rc};
const DEFAULT_SERVER: &str = "https://remoteidcertisign.com.br";
pub(crate) mod about;
pub(crate) mod browser_panel;
fn prompt_action(registered: bool) -> Result<Action> {
    if registered {
        return Ok(Action::Refresh);
    }
    let fields = [
        crate::ui::Field {
            label: "E-mail",
            secret: false,
            required: true,
        },
        crate::ui::Field {
            label: "Senha do aplicativo Certisign",
            secret: true,
            required: true,
        },
        crate::ui::Field {
            label: "Nome deste computador (opcional)",
            secret: false,
            required: false,
        },
    ];
    let Some(mut values) = crate::ui::form(
        "Axia RemotedID — Conectar conta",
        "Entre com os dados do aplicativo Certisign. Ao conectar, este computador será cadastrado uma vez na sua conta.",
        &fields,
        "Conectar e cadastrar",
        None,
    ) else {
        return Ok(Action::Cancel);
    };
    let desktop_name = values.pop().context("Nome ausente")?.to_string();
    let password = values.pop().context("Senha ausente")?;
    let email = values.pop().context("E-mail ausente")?.trim().to_owned();
    Ok(Action::Register(Credentials {
        email,
        password,
        desktop_name,
    }))
}

fn error_message(error: &anyhow::Error) -> String {
    if let Some(status) = crate::client::service_status(error) {
        format!("{}\n\nO serviço respondeu HTTP {}.", error, status)
    } else {
        error.to_string()
    }
}

struct ConnectionState {
    registered: bool,
    certificate_count: Option<usize>,
    identity_exists: bool,
}

impl ConnectionState {
    fn read(store: &Store) -> Result<Self> {
        let identity_exists = store.path().try_exists()?;
        let registered = identity_exists && store.load()?.0.desktop_code.is_some();
        let certificate_count = if registered {
            store.load_wallet_public().ok().map(|wallet| wallet.len())
        } else {
            None
        };
        Ok(Self {
            registered,
            certificate_count,
            identity_exists,
        })
    }

    fn connect_label(&self) -> &'static str {
        if self.registered {
            "Atualizar certificados"
        } else {
            "Conectar conta"
        }
    }

    fn status(&self) -> String {
        match (self.registered, self.certificate_count) {
            (true, Some(count)) => format!(
                "Computador cadastrado. Certificados salvos: {count}.\nAtualize os certificados ou configure os navegadores instalados."
            ),
            (true, None) => "Computador cadastrado. Atualize os certificados para disponibilizá-los e configurar os navegadores.".into(),
            (false, _) if self.identity_exists => "A identidade local está salva; o cadastro ainda não foi confirmado.".into(),
            (false, _) => "Conecte sua conta para disponibilizar os certificados.".into(),
        }
    }
}

fn connection_action(store: &Store, prompt: impl FnOnce(bool) -> Result<Action>) -> Result<Action> {
    // Read again at the time of the click. Never decide to register using a
    // flag from before a worker persisted the registration code.
    let state = ConnectionState::read(store)?;
    if state.identity_exists && !state.registered {
        store.ensure_registration_available()?;
    }
    prompt(state.registered)
}

fn complete_connection(
    store: &Store,
    result: Result<Option<ConnectionSummary>>,
    connect: &gtk::Button,
    browsers: &gtk::Button,
    status: &gtk::Label,
) {
    // The worker may have saved a registration even when its final result is
    // an error. Refresh controls after success, error and cancellation alike.
    let state = match ConnectionState::read(store) {
        Ok(state) => state,
        Err(error) => {
            connect.set_sensitive(false);
            browsers.set_sensitive(false);
            let operation = result
                .err()
                .map(|error| format!("{}\n\n", error_message(&error)))
                .unwrap_or_default();
            status.set_text(&format!(
                "{operation}Não foi possível consultar o estado local: {}",
                error_message(&error)
            ));
            return;
        }
    };
    connect.set_label(state.connect_label());
    connect.set_sensitive(true);
    browsers.set_sensitive(state.certificate_count.is_some());
    match result {
        Ok(Some(summary)) => status.set_text(&format!("{}\n{}\nCertificados disponíveis: {}. Compatíveis com assinatura: {}.\nConfigure os navegadores para usar seus certificados.", if summary.registered_now { "Conta conectada e computador cadastrado." } else { "Cadastro existente reutilizado." }, if summary.wallet_updated { "Certificados atualizados." } else { "A carteira já está atualizada." }, summary.certificate_count, summary.compatible_count)),
        Ok(None) => status.set_text(&state.status()),
        Err(error) => {
            let recovery = if state.registered {
                "\n\nO cadastro deste computador está salvo. Clique em “Atualizar certificados” para consultar novamente, sem novo cadastro."
            } else { "" };
            status.set_text(&format!("{}{recovery}", error_message(&error)));
        }
    }
}

pub fn run(store: &Store) -> Result<()> {
    crate::ui::init()?;
    crate::desktop::user()?;
    let home = crate::desktop::home()?;
    let _ = crate::desktop::migrate_shortcuts(&home);
    crate::autostart::first_run(
        &crate::autostart::config_root()?,
        &crate::autostart::executable()?,
        store.path(),
        || {
            Ok(crate::ui::confirm(
                "Deseja iniciar o Axia RemotedID com o sistema?\nEle ficará na bandeja, sem abrir uma janela. Você pode alterar essa opção depois.",
                "Ativar",
                "Agora não",
            ))
        },
    )?;
    if let Err(error) = crate::tray::start_background(store) {
        eprintln!("Não foi possível iniciar a bandeja: {error}");
    }
    let state = ConnectionState::read(store)?;
    let busy = Rc::new(Cell::new(false));
    let window = gtk::Window::builder()
        .title("Axia RemotedID — Certificados em nuvem")
        .default_width(600)
        .build();
    window.set_icon_name(Some("dialog-password"));
    let area = gtk::Box::new(gtk::Orientation::Vertical, 18);
    area.set_margin_top(28);
    area.set_margin_bottom(28);
    area.set_margin_start(28);
    area.set_margin_end(28);
    let title = gtk::Label::new(Some("Seus certificados no Linux"));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    area.append(&title);
    let status = gtk::Label::new(Some(&state.status()));
    status.set_wrap(true);
    status.set_xalign(0.0);
    area.append(&status);
    let connect = gtk::Button::with_label(state.connect_label());
    connect.add_css_class("suggested-action");
    area.append(&connect);
    let browsers = gtk::Button::with_label("Configurar navegadores");
    browsers.set_sensitive(state.certificate_count.is_some());
    area.append(&browsers);
    let browser_panel = browser_panel::BrowserPanel::new();
    area.append(&browser_panel.widget);
    if crate::browsers::module(&home).is_err() {
        let install = gtk::Button::with_label("Instalar para este usuário");
        area.append(&install);
        let home = home.clone();
        let status = status.clone();
        install.connect_clicked(move |button| {
            let result = (|| {
                let target = std::env::current_exe()?
                    .parent()
                    .context("Pasta da build ausente")?
                    .to_owned();
                let home = home.clone();
                crate::ui::work("Instalando o aplicativo para este usuário...", move |_| {
                    crate::desktop::install(&target, &home)
                })
            })();
            match result {
                Ok(_) => {
                    status.set_text(
                        "Aplicativo instalado. Conecte sua conta e configure os navegadores.",
                    );
                    button.set_sensitive(false);
                }
                Err(error) => status.set_text(&error_message(&error)),
            }
        });
    }
    let close = gtk::Button::with_label("Fechar");
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.set_homogeneous(true);
    footer.append(&about::button(&window));
    footer.append(&close);
    area.append(&footer);
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    let root = store.path().to_owned();
    let busy_action = busy.clone();
    let area_action = area.clone();
    let status_action = status.clone();
    let browsers_action = browsers.clone();
    connect.connect_clicked(move |button| {
        if busy_action.replace(true) {
            return;
        }
        area_action.set_sensitive(false);
        let result: Result<Option<ConnectionSummary>> = (|| {
            let store = Store::new(root.clone())?;
            let action = connection_action(&store, prompt_action)?;
            if matches!(action, Action::Cancel) {
                return Ok(None);
            }
            let root = root.clone();
            crate::ui::work("Conectando sua conta...", move |progress| {
                let store = Store::new(root)?;
                let server = if store.path().exists() {
                    store.load()?.0.server
                } else {
                    DEFAULT_SERVER.into()
                };
                let api = Api::new(&server)?;
                connect_with(
                    &api,
                    &store,
                    |_| Ok(action),
                    |text| {
                        progress
                            .send(text.to_owned())
                            .context("Interface indisponível")
                    },
                )
            })
        })();
        match Store::new(root.clone()) {
            Ok(store) => {
                complete_connection(&store, result, button, &browsers_action, &status_action)
            }
            Err(error) => {
                button.set_sensitive(false);
                browsers_action.set_sensitive(false);
                status_action.set_text(&error_message(&error));
            }
        }
        area_action.set_sensitive(true);
        busy_action.set(false);
    });
    let root = store.path().to_owned();
    let busy_action = busy.clone();
    let area_action = area.clone();
    let browser_home = home.clone();
    browsers.connect_clicked(move |_| {
        if busy_action.replace(true) {
            return;
        }
        area_action.set_sensitive(false);
        browser_panel.complete(
            Store::new(root.clone()).and_then(|store| crate::browsers::setup_ui(&store, None)),
            &browser_home,
        );
        area_action.set_sensitive(true);
        busy_action.set(false);
    });
    let loop_ = glib::MainLoop::new(None, false);
    let end = loop_.clone();
    window.connect_close_request(move |_| {
        if busy.get() {
            glib::Propagation::Stop
        } else {
            end.quit();
            glib::Propagation::Proceed
        }
    });
    window.set_child(Some(&area));
    window.present();
    loop_.run();
    window.destroy();
    Ok(())
}

/// Autostart is local and windowless. GTK is not initialized on this route.
pub fn run_startup(store: &Store) -> Result<()> {
    if !crate::autostart::enabled(&crate::autostart::config_root()?)? {
        return Ok(());
    }
    crate::tray::run(store)
}

#[cfg(test)]
#[path = "gui/connection_tests.rs"]
pub(crate) mod connection_tests;
