//! Native GTK widgets. GTK stays on the UI thread; slow operations use workers.
//! Credentials are never written to stdout, argv, environment or disk.
use anyhow::{Context, Result, ensure};
use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
use zeroize::Zeroizing;

pub fn init() -> Result<()> {
    ensure!(
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        "Sessão gráfica indisponível. Execute em uma sessão gráfica ou use o terminal"
    );
    gtk::init().context("Não foi possível abrir a interface gráfica")?;
    static NAME: std::sync::Once = std::sync::Once::new();
    NAME.call_once(|| glib::set_application_name("Axia RemotedID"));
    Ok(())
}

fn content(title: &str, text: &str) -> (gtk::Window, gtk::Box) {
    let window = gtk::Window::builder()
        .title(title)
        .default_width(540)
        .modal(true)
        .build();
    window.set_icon_name(Some("dialog-password"));
    let area = gtk::Box::new(gtk::Orientation::Vertical, 14);
    area.set_margin_top(24);
    area.set_margin_bottom(24);
    area.set_margin_start(24);
    area.set_margin_end(24);
    let label = gtk::Label::new(Some(&text.replace('\0', "�")));
    label.set_wrap(true);
    label.set_xalign(0.0);
    area.append(&label);
    window.set_child(Some(&area));
    (window, area)
}

pub struct Field<'a> {
    pub label: &'a str,
    pub secret: bool,
    pub required: bool,
}

struct Form {
    window: gtk::Window,
    fields: Vec<gtk::Entry>,
    accept: gtk::Button,
    accepted: Rc<Cell<bool>>,
    loop_: glib::MainLoop,
}

fn prepare_form(
    title: &str,
    text: &str,
    fields: &[Field<'_>],
    accept: &str,
    cancel: Option<&str>,
) -> Form {
    let (window, area) = content(title, text);
    let loop_ = glib::MainLoop::new(None, false);
    let accepted = Rc::new(Cell::new(false));
    let entries: Vec<_> = fields
        .iter()
        .map(|field| {
            let label = gtk::Label::new(Some(field.label));
            label.set_xalign(0.0);
            let entry = gtk::Entry::builder()
                .visibility(!field.secret)
                .hexpand(true)
                .build();
            if field.secret {
                entry.set_buffer(&gtk::PasswordEntryBuffer::new());
                entry.set_input_purpose(gtk::InputPurpose::Password);
                entry.set_secondary_icon_name(Some("view-reveal-symbolic"));
                entry.connect_icon_press(|entry, position| {
                    if position == gtk::EntryIconPosition::Secondary {
                        entry.set_visibility(!gtk::prelude::EntryExt::is_visible(entry));
                    }
                });
            }
            area.append(&label);
            area.append(&entry);
            entry
        })
        .collect();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::End);
    if let Some(cancel) = cancel {
        let cancel = gtk::Button::with_label(cancel);
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
        buttons.append(&cancel);
    }
    let button = gtk::Button::with_label(accept);
    button.add_css_class("suggested-action");
    let result = accepted.clone();
    let close = window.clone();
    button.connect_clicked(move |_| {
        result.set(true);
        close.close();
    });
    buttons.append(&button);
    area.append(&buttons);
    let required: Vec<_> = fields
        .iter()
        .zip(&entries)
        .filter(|(field, _)| field.required)
        .map(|(_, entry)| entry.clone())
        .collect();
    button.set_sensitive(required.is_empty());
    for entry in &required {
        let required = required.clone();
        let button = button.clone();
        entry.connect_changed(move |_| {
            button.set_sensitive(required.iter().all(|entry| !entry.text().is_empty()))
        });
    }
    window.set_default_widget(Some(&button));
    for entry in &entries {
        entry.set_activates_default(true);
    }
    let close_loop = loop_.clone();
    window.connect_close_request(move |_| {
        close_loop.quit();
        glib::Propagation::Proceed
    });
    Form {
        window,
        fields: entries,
        accept: button,
        accepted,
        loop_,
    }
}

impl Form {
    fn wait(self, timeout: Option<Duration>) -> Option<Vec<Zeroizing<String>>> {
        let expired = Rc::new(Cell::new(false));
        let source = timeout.map(|timeout| {
            let window = self.window.clone();
            let expired = expired.clone();
            glib::timeout_add_local_once(timeout, move || {
                expired.set(true);
                window.close();
            })
        });
        self.window.present();
        if let Some(entry) = self.fields.first() {
            entry.grab_focus();
        }
        self.loop_.run();
        if !expired.get()
            && let Some(source) = source
        {
            source.remove();
        }
        let result = self.accepted.get().then(|| {
            self.fields
                .iter()
                .map(|entry| Zeroizing::new(entry.text().to_string()))
                .collect()
        });
        for entry in &self.fields {
            entry.set_text("");
        }
        self.window.set_child(None::<&gtk::Widget>);
        self.window.destroy();
        result
    }
}

pub fn form(
    title: &str,
    text: &str,
    fields: &[Field<'_>],
    accept: &str,
    timeout: Option<Duration>,
) -> Option<Vec<Zeroizing<String>>> {
    prepare_form(title, text, fields, accept, Some("Cancelar")).wait(timeout)
}
pub fn confirm(text: &str, accept: &str, cancel: &str) -> bool {
    prepare_form("Axia RemotedID", text, &[], accept, Some(cancel))
        .wait(None)
        .is_some()
}
pub fn message(text: &str) {
    let _ = prepare_form("Axia RemotedID", text, &[], "OK", None).wait(None);
}

pub fn choose(text: &str, choices: &[String]) -> Option<Vec<usize>> {
    let form = prepare_form(
        "Axia RemotedID — Configurar navegadores",
        text,
        &[],
        "Configurar selecionados",
        Some("Agora não"),
    );
    let area = form.window.child().unwrap().downcast::<gtk::Box>().unwrap();
    let checkboxes: Vec<_> = choices
        .iter()
        .map(|label| {
            let checkbox = gtk::CheckButton::with_label(label);
            checkbox.set_active(true);
            area.insert_child_after(&checkbox, area.first_child().as_ref());
            checkbox
        })
        .collect();
    for checkbox in &checkboxes {
        let checkboxes = checkboxes.clone();
        let button = form.accept.clone();
        checkbox.connect_toggled(move |_| {
            button.set_sensitive(checkboxes.iter().any(gtk::CheckButton::is_active))
        });
    }
    form.accept.set_sensitive(!choices.is_empty());
    form.wait(None)?;
    Some(
        checkboxes
            .iter()
            .enumerate()
            .filter(|(_, checkbox)| checkbox.is_active())
            .map(|(index, _)| index)
            .collect(),
    )
}

/// Present progress while a worker runs; closing cannot interrupt an ambiguous
/// remote registration. Only explicitly initiated operations reach this point.
pub fn work<T: Send + 'static>(
    text: &str,
    job: impl FnOnce(mpsc::Sender<String>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let (window, area) = content("Axia RemotedID", text);
    let progress = gtk::Label::new(Some(text));
    progress.set_wrap(true);
    progress.set_xalign(0.0);
    let spinner = gtk::Spinner::new();
    spinner.start();
    area.append(&spinner);
    area.append(&progress);
    let loop_ = glib::MainLoop::new(None, false);
    let (updates, update_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(job(updates));
    });
    let result = Rc::new(RefCell::new(None));
    let completed = Rc::new(Cell::new(false));
    let complete = completed.clone();
    window.connect_close_request(move |_| {
        if complete.get() {
            glib::Propagation::Proceed
        } else {
            glib::Propagation::Stop
        }
    });
    let output = result.clone();
    let close_loop = loop_.clone();
    glib::timeout_add_local(Duration::from_millis(40), move || {
        for text in update_rx.try_iter() {
            progress.set_text(&text);
        }
        let value = match done_rx.try_recv() {
            Ok(value) => value,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!(
                "Operação interrompida; o estado local foi preservado"
            )),
        };
        *output.borrow_mut() = Some(value);
        completed.set(true);
        close_loop.quit();
        glib::ControlFlow::Break
    });
    window.present();
    loop_.run();
    window.destroy();
    result
        .borrow_mut()
        .take()
        .context("Resultado da operação ausente")?
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Run explicitly under Broadway or Xvfb; no account or network is used.
    #[test]
    #[ignore = "requires isolated graphical display"]
    fn native_forms_clear_passwords_cancel_timeout_and_keep_worker_responsive() {
        init().unwrap();
        crate::gui::connection_tests::exercise_connection_recovery();
        if unsafe { libc::geteuid() } == 0 {
            let temporary = tempfile::tempdir().unwrap();
            let state = temporary.path().join("must-not-create-root-account");
            let store = crate::storage::Store::new(state.clone()).unwrap();
            let error = crate::gui::run(&store).unwrap_err();
            assert!(error.to_string().contains("usuário normal"));
            assert!(!state.exists());
        }
        let fields = [Field {
            label: "Senha de laboratório",
            secret: true,
            required: true,
        }];
        let form = prepare_form(
            "Laboratório Axia",
            "Sem conta real",
            &fields,
            "Continuar",
            Some("Cancelar"),
        );
        let entry = form.fields[0].clone();
        let button = form.accept.clone();
        let check = entry.clone();
        assert!(!button.is_sensitive());
        glib::timeout_add_local_once(Duration::from_millis(20), move || {
            entry.set_text("  LAB | value  ");
            assert!(button.is_sensitive());
            button.emit_clicked();
        });
        let result = form.wait(None).unwrap();
        assert_eq!(result[0].as_str(), "  LAB | value  ");
        assert!(check.text().is_empty());
        let form = prepare_form(
            "Laboratório Axia",
            "Cancelar sem operação",
            &fields,
            "Continuar",
            Some("Cancelar"),
        );
        let close = form.window.clone();
        glib::timeout_add_local_once(Duration::from_millis(20), move || close.close());
        assert!(form.wait(None).is_none());
        assert!(
            prepare_form(
                "Laboratório Axia",
                "Expiração",
                &fields,
                "Continuar",
                Some("Cancelar")
            )
            .wait(Some(Duration::from_millis(20)))
            .is_none()
        );
        let responsive = Rc::new(Cell::new(false));
        let flag = responsive.clone();
        glib::timeout_add_local_once(Duration::from_millis(20), move || flag.set(true));
        let result = work("Operação de laboratório", |progress| {
            progress.send("Progresso sintético".into()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            Ok(42)
        })
        .unwrap();
        assert_eq!(result, 42);
        assert!(responsive.get());
    }
}
