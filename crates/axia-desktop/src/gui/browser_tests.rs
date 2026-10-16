use super::*;
use crate::browsers::{
    Family, MODULE_NAME, Registration, Target,
    report::{FailureKind, Recovery, RecoveryState, SetupStatus, TargetStatus},
};

fn entry(path: &str, configured: bool) -> TargetResult {
    let target = Target {
        family: Family::Firefox,
        label: "Firefox — Trabalho".into(),
        database: path.into(),
        create: false,
        firefox_default: None,
        installations: Vec::new(),
    };
    let result = if configured {
        Ok(Registration {
            browser: target.family,
            profile: target.database.clone(),
            module: MODULE_NAME,
            registered_now: false,
            migrated_legacy_module: false,
            module_loaded_by_nss: true,
            certificate_count: 2,
            backup_directory: None,
            remote_signature_requested: false,
        })
    } else {
        Err(anyhow::anyhow!("NSS laboratory error -8018"))
    };
    TargetResult::from_registration(target, result)
}

/// Called by the existing isolated-display test on the same GTK thread.
pub(crate) fn exercise_browser_results() {
    let home = Path::new("/laboratory");
    let panel = BrowserPanel::new();
    let connection_status = gtk::Label::new(Some("Cadastro salvo. Atualizar certificados."));
    let window = gtk::Window::new();
    let area = gtk::Box::new(gtk::Orientation::Vertical, 8);
    area.append(&connection_status);
    area.append(&panel.widget);
    window.set_child(Some(&area));
    window.present();
    panel.complete(
        Ok(SetupReport::from_results(vec![
            entry("/laboratory/first", true),
            entry("/laboratory/second", false),
        ])),
        home,
    );
    assert!(panel.summary.text().contains("parcial"));
    let rows = || {
        let mut rows = Vec::new();
        let mut child = panel.entries.first_child();
        while let Some(widget) = child {
            rows.push(
                widget
                    .clone()
                    .downcast::<gtk::Label>()
                    .unwrap()
                    .text()
                    .to_string(),
            );
            child = widget.next_sibling();
        }
        rows
    };
    assert_eq!(rows().len(), 2);
    assert!(rows()[0].contains("~/first\nConfigurado"));
    assert!(rows()[1].contains("~/second\nFalha"));
    assert!(rows()[1].contains("NSS laboratory error -8018"));
    // A retry replaces precisely the matching profile, keeping the other row.
    panel.complete(
        Ok(SetupReport::from_results(vec![entry(
            "/laboratory/second",
            true,
        )])),
        home,
    );
    assert_eq!(rows().len(), 2);
    assert!(rows().iter().all(|row| row.contains("Configurado")));
    assert_eq!(panel.results.borrow()[0].status, TargetStatus::Configured);
    for status in [SetupStatus::Cancelled, SetupStatus::NoTargets] {
        let previous = rows();
        panel.complete(
            Ok(SetupReport {
                schema_version: 1,
                status,
                results: Vec::new(),
                notices: Vec::new(),
            }),
            home,
        );
        assert_eq!(rows(), previous);
        assert!(!panel.summary.text().contains("Reabra"));
        assert!(panel.summary.text().contains("anteriores"));
    }
    let previous = rows();
    panel.complete(Err(anyhow::anyhow!("synthetic worker error")), home);
    assert_eq!(rows(), previous);
    assert!(panel.summary.text().contains("synthetic worker error"));
    // Latest failed attempt cannot leave that target looking configured.
    panel.complete(
        Ok(SetupReport::from_results(vec![entry(
            "/laboratory/second",
            false,
        )])),
        home,
    );
    assert!(rows()[0].contains("Configurado"));
    assert!(rows()[1].contains("Falha"));
    assert!(!panel.summary.text().contains("Reabra"));
    let blocked = TargetResult::from_registration(
        entry("/laboratory/second", false).target,
        Err(anyhow::anyhow!("NSS laboratory cause").context(Recovery {
            state: RecoveryState::Suspended,
            backup_directory: "/laboratory/backup".into(),
            reason: Some(FailureKind::BrowserOpen),
            detail: "Restauração automática suspensa: feche os navegadores; backup preservado"
                .into(),
        })),
    );
    panel.complete(Ok(SetupReport::from_results(vec![blocked])), home);
    assert!(rows()[1].contains("Ação necessária"));
    assert!(rows()[1].contains("Restauração automática suspensa"));
    assert!(rows()[1].contains("~/backup"));
    assert!(!rows()[1].contains("foi restaurada"));
    assert!(rows()[0].contains("Configurado"));
    assert_eq!(
        connection_status.text(),
        "Cadastro salvo. Atualizar certificados."
    );
    window.destroy();
    let new_panel = BrowserPanel::new();
    assert!(new_panel.results.borrow().is_empty());
    assert!(new_panel.summary.text().contains("Nenhuma configuração"));
}
