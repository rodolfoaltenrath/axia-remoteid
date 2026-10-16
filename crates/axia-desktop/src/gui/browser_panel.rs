use crate::browsers::report::{SetupReport, TargetResult};
use gtk::prelude::*;
use std::{cell::RefCell, path::Path};

/// Last result per target, held only for the lifetime of this panel.
pub(super) struct BrowserPanel {
    pub widget: gtk::Box,
    summary: gtk::Label,
    entries: gtk::Box,
    results: RefCell<Vec<TargetResult>>,
}
impl BrowserPanel {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let heading = gtk::Label::new(Some("Configuração dos navegadores"));
        heading.add_css_class("heading");
        heading.set_xalign(0.0);
        widget.append(&heading);
        let summary = label(
            "Nenhuma configuração solicitada nesta sessão. Os resultados ficam disponíveis enquanto este painel estiver aberto.",
        );
        widget.append(&summary);
        let entries = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(240)
            .propagate_natural_height(true)
            .child(&entries)
            .build();
        widget.append(&scroll);
        Self {
            widget,
            summary,
            entries,
            results: RefCell::new(Vec::new()),
        }
    }
    pub fn complete(&self, result: anyhow::Result<SetupReport>, home: &Path) {
        match result {
            Ok(report) => {
                let mut results = self.results.borrow_mut();
                let no_attempts = report.results.is_empty();
                for entry in &report.results {
                    if let Some(previous) = results
                        .iter_mut()
                        .find(|previous| previous.same_target(entry))
                    {
                        *previous = entry.clone();
                    } else {
                        results.push(entry.clone());
                    }
                }
                let previous = if no_attempts && !results.is_empty() {
                    "\nOs resultados das tentativas anteriores continuam abaixo."
                } else { "" };
                self.summary.set_text(&format!(
                    "{}{previous}\nResultados da sessão deste painel; não são uma verificação contínua dos navegadores.", report.summary()
                ));
                while let Some(child) = self.entries.first_child() {
                    self.entries.remove(&child);
                }
                for entry in results.iter() {
                    self.entries.append(&label(&entry.text(home)));
                }
            }
            Err(error) => self.summary.set_text(&format!("Não foi possível concluir a configuração: {error:#}\nOs resultados anteriores continuam abaixo. Corrija o problema e tente novamente.")),
        }
    }
}
fn label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_selectable(true);
    label
}

#[cfg(test)]
#[path = "browser_tests.rs"]
pub(crate) mod tests;
