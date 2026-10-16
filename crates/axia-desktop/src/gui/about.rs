use gtk::prelude::*;

fn dialog(parent: &gtk::Window) -> gtk::AboutDialog {
    gtk::AboutDialog::builder()
        .title("Sobre o Axia RemotedID")
        .program_name("Axia RemotedID")
        .version(env!("CARGO_PKG_VERSION"))
        .logo_icon_name("dialog-password")
        .comments(concat!(
            "Certificados digitais em nuvem no Linux.\n\n",
            "Senhas e códigos temporários não são gravados nem guardados no cache de autorização.\n\n",
            "Apenas o token de autorização permanece em memória por até 10 minutos, ",
            "no mesmo aplicativo e certificado. O prazo não se renova com o uso. ",
            "Fechar o navegador ou o aplicativo que utiliza o certificado encerra essa autorização. ",
            "O serviço pode exigir uma nova autorização antes do prazo.\n\n",
            "Projeto independente, sem vínculo oficial com a Certisign."
        ))
        .license_type(gtk::License::Apache20)
        .website("https://github.com/rodolfoaltenrath/axia-remoteid")
        .website_label("Página do projeto")
        .transient_for(parent)
        .modal(true)
        .hide_on_close(false)
        .destroy_with_parent(true)
        .build()
}

pub(super) fn button(parent: &gtk::Window) -> gtk::Button {
    let button = gtk::Button::with_label("Sobre");
    let parent = parent.downgrade();
    button.connect_clicked(move |_| {
        if let Some(parent) = parent.upgrade() {
            dialog(&parent).present();
        }
    });
    button
}

#[cfg(test)]
pub(crate) fn exercise_about() {
    let parent = gtk::Window::new();
    let button = button(&parent);
    parent.set_child(Some(&button));
    parent.present();
    button.emit_clicked();
    let about = gtk::Window::toplevels()
        .iter::<gtk::glib::Object>()
        .filter_map(Result::ok)
        .find_map(|window| window.downcast::<gtk::AboutDialog>().ok())
        .expect("Sobre must open from its main-panel button");
    assert_eq!(about.transient_for(), Some(parent.clone()));
    assert!(about.is_modal());
    assert_eq!(about.version().as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert!(
        about
            .comments()
            .unwrap()
            .contains("Senhas e códigos temporários")
    );
    about.close();
    assert!(!about.is_visible());
    // Reopening and closing the parent must not leave an orphan About window.
    button.emit_clicked();
    let about = gtk::Window::toplevels()
        .iter::<gtk::glib::Object>()
        .filter_map(Result::ok)
        .find_map(|window| window.downcast::<gtk::AboutDialog>().ok())
        .unwrap();
    assert!(about.surface().is_some());
    parent.destroy();
    drop(parent);
    assert!(about.surface().is_none());
}
