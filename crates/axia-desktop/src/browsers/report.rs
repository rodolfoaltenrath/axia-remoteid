//! Local configuration results. No cloud operation or credential enters this report.
use super::{Registration, Target};
use anyhow::Result;
use serde::Serialize;
use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    BrowserOpen,
    ProfileBusy,
    InspectionUnavailable,
    ProfileNotInitialized,
    Configuration,
}

#[derive(Debug)]
pub(super) struct Blocked(pub FailureKind);
impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            FailureKind::BrowserOpen => "Feche os navegadores baseados em Chromium antes de configurar os certificados",
            FailureKind::ProfileBusy => "Feche o navegador e aguarde o fim de outras configurações: a base de certificados está em uso",
            FailureKind::InspectionUnavailable => "Não foi possível verificar se os navegadores estão fechados; tente novamente após corrigir a inspeção de processos",
            FailureKind::ProfileNotInitialized => "Este perfil ainda não está preparado para certificados. Abra-o no navegador uma vez e feche-o antes de configurar",
            FailureKind::Configuration => "Não foi possível configurar a base de certificados",
        })
    }
}
impl std::error::Error for Blocked {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    NotNeeded,
    Restored,
    Suspended,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Recovery {
    pub state: RecoveryState,
    pub backup_directory: PathBuf,
    pub reason: Option<FailureKind>,
    pub detail: String,
}
impl fmt::Display for Recovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}
impl std::error::Error for Recovery {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Configured,
    NeedsAction,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    pub recovery: Option<Recovery>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetResult {
    pub target: Target,
    pub status: TargetStatus,
    pub registration: Option<Registration>,
    pub failure: Option<Failure>,
}
impl TargetResult {
    pub fn from_registration(mut target: Target, result: Result<Registration>) -> Self {
        match result {
            Ok(registration) => {
                target.database = registration.profile.clone();
                Self {
                    target,
                    status: TargetStatus::Configured,
                    registration: Some(registration),
                    failure: None,
                }
            }
            Err(error) => {
                let recovery = error.downcast_ref::<Recovery>().cloned();
                let kind = error
                    .downcast_ref::<Blocked>()
                    .map(|blocked| blocked.0)
                    .or_else(|| recovery.as_ref().and_then(|value| value.reason))
                    .unwrap_or(FailureKind::Configuration);
                Self {
                    target,
                    status: if kind == FailureKind::Configuration {
                        TargetStatus::Failed
                    } else {
                        TargetStatus::NeedsAction
                    },
                    registration: None,
                    failure: Some(Failure {
                        kind,
                        message: format!("{error:#}"),
                        recovery,
                    }),
                }
            }
        }
    }

    pub fn text(&self, home: &Path) -> String {
        let detail = if let Some(registration) = &self.registration {
            format!(
                "Configurado; {} certificado(s) disponível(is).{}",
                registration.certificate_count,
                if registration.registered_now {
                    ""
                } else {
                    " Registro existente reutilizado."
                }
            )
        } else if let Some(failure) = &self.failure {
            let backup = failure
                .recovery
                .as_ref()
                .map(|recovery| {
                    format!(
                        "\nBackup privado preservado: {}",
                        display_path(&recovery.backup_directory, home)
                    )
                })
                .unwrap_or_default();
            format!(
                "{}: {}{backup}",
                if self.status == TargetStatus::NeedsAction {
                    "Ação necessária"
                } else {
                    "Falha"
                },
                failure.message
            )
        } else {
            "Resultado indisponível.".into()
        };
        format!(
            "{}\n{}\n{detail}",
            self.target.label,
            display_path(&self.target.database, home)
        )
    }

    pub fn same_target(&self, other: &Self) -> bool {
        self.target.family == other.target.family && self.target.database == other.target.database
    }
}

fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Configured,
    Partial,
    Failed,
    Cancelled,
    NoTargets,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupReport {
    pub schema_version: u8,
    pub status: SetupStatus,
    pub results: Vec<TargetResult>,
    pub notices: Vec<String>,
}
impl SetupReport {
    pub(super) fn empty(status: SetupStatus, notices: Vec<String>) -> Self {
        Self {
            schema_version: 1,
            status,
            results: Vec::new(),
            notices,
        }
    }
    pub fn from_results(results: Vec<TargetResult>) -> Self {
        let count = results
            .iter()
            .filter(|entry| entry.status == TargetStatus::Configured)
            .count();
        let status = if results.is_empty() {
            SetupStatus::NoTargets
        } else if count == results.len() {
            SetupStatus::Configured
        } else if count > 0 {
            SetupStatus::Partial
        } else {
            SetupStatus::Failed
        };
        Self {
            schema_version: 1,
            status,
            results,
            notices: Vec::new(),
        }
    }
    pub fn successful(&self) -> bool {
        self.status == SetupStatus::Configured
    }
    pub fn summary(&self) -> String {
        let count = self
            .results
            .iter()
            .filter(|entry| entry.status == TargetStatus::Configured)
            .count();
        let text = match self.status {
            SetupStatus::Configured => format!("{count} alvo(s) configurado(s)."),
            SetupStatus::Partial => format!("Configuração parcial: {count} alvo(s) configurado(s); {} precisa(m) de ação. Corrija os problemas abaixo e tente novamente nesses alvos.", self.results.len() - count),
            SetupStatus::Failed => "Nenhum alvo foi configurado nesta tentativa. Corrija os problemas abaixo e clique em “Configurar navegadores” para tentar novamente.".into(),
            SetupStatus::Cancelled => "Configuração cancelada; nenhum registro foi solicitado.".into(),
            SetupStatus::NoTargets => "Nenhum alvo disponível para configurar.".into(),
        };
        let reopen = if count > 0 {
            "\nReabra os navegadores configurados e escolha a entrada com certificado digital no serviço desejado."
        } else {
            ""
        };
        let notices = if self.notices.is_empty() {
            String::new()
        } else {
            format!("\n{}", self.notices.join("\n"))
        };
        format!("{text}{reopen}{notices}")
    }
    pub fn message(&self, home: &Path) -> String {
        let entries = self
            .results
            .iter()
            .map(|entry| entry.text(home))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "{}{}{}",
            self.summary(),
            if entries.is_empty() { "" } else { "\n\n" },
            entries
        )
    }
}

/// Each selected target is attempted once. Failure cannot discard earlier results.
pub fn configure(
    targets: Vec<Target>,
    module: &Path,
    home: &Path,
    progress: impl FnMut(&Target),
) -> SetupReport {
    configure_with(targets, progress, |target| {
        super::register(target, module, home, true)
    })
}
pub(super) fn configure_with(
    targets: Vec<Target>,
    mut progress: impl FnMut(&Target),
    mut register: impl FnMut(&Target) -> Result<Registration>,
) -> SetupReport {
    let mut results: Vec<TargetResult> = Vec::new();
    for target in targets {
        if results.iter().any(|entry| {
            entry.target.family == target.family && entry.target.database == target.database
        }) {
            continue;
        }
        progress(&target);
        let result = register(&target);
        results.push(TargetResult::from_registration(target, result));
    }
    SetupReport::from_results(results)
}

#[cfg(test)]
mod tests;
