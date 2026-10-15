//! Developer-only tools. This executable is never included in the runtime RPM.
mod chromium_smoke;
mod cli_smoke;
mod diagnostic;
mod firefox_smoke;
mod fixtures;
mod lab;
mod lab_helper;
mod native_ui;
mod nss;
mod package;
mod pkcs;
mod pkcs_smoke;
mod publication;
mod rpm_smoke;
mod sign_test;
mod tray_smoke;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "Ferramentas compiladas de desenvolvimento do Axia RemotedID"
)]
struct Cli {
    /// Árvore de fontes (padrão: diretório de trabalho).
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Verifica candidatos à publicação sem exibir conteúdo privado.
    CheckPublication,
    /// Gera ZIP reproduzível somente com fontes aprovadas pelo verificador.
    PackageSources {
        #[arg(long, default_value = "dist/desktopid-linux-source.zip")]
        output: PathBuf,
    },
    /// Gera tar.gz com os ELF, documentação e manifesto de hashes.
    PackageLinux {
        #[arg(long, default_value = "dist/fedora")]
        target: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Gera RPM Fedora 44 x86_64; requer rpmbuild ou Podman no desenvolvedor.
    PackageRpm {
        #[arg(long, default_value = "dist/fedora")]
        target: PathBuf,
        #[arg(long, default_value = "dist")]
        output_dir: PathBuf,
        #[arg(long, env = "RPMBUILD", default_value = "rpmbuild")]
        rpmbuild: PathBuf,
        #[arg(long)]
        container: bool,
        #[arg(long, default_value = "registry.fedoraproject.org/fedora:44")]
        container_image: String,
    },
    /// Inventário local de ambiente e ELF, sem carregar o módulo ou acessar conta.
    FedoraDiagnostic {
        #[arg(long)]
        module: Option<PathBuf>,
    },
    /// Gera novo fixture sintético; não substitui o fixture revisado do projeto.
    GenerateVectors {
        #[arg(long)]
        output: PathBuf,
    },
    /// Testa o CLI com fixtures públicos e home isolado, sem rede de conta.
    CliSmoke {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Testa GTK em Broadway e D-Bus privados, sem sessão gráfica do usuário.
    NativeUiSmoke {
        #[arg(long, default_value = "target")]
        target: PathBuf,
    },
    /// Enumera contagens locais do módulo Axia sem solicitar assinatura.
    Pkcs11Inventory {
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Teste interativo: solicita uma única assinatura real de laboratório.
    SignPkcs11Test {
        #[arg(long, default_value_t = 1)]
        certificate_index: usize,
        #[arg(long)]
        inventory_only: bool,
        #[arg(long)]
        module: Option<PathBuf>,
        #[arg(long)]
        helper: Option<PathBuf>,
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    /// Testa ABI, assinatura sintética, cancelamento e OpenSC sem nuvem.
    Pkcs11Smoke {
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    #[command(hide = true)]
    Pkcs11Worker {
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        home: PathBuf,
    },
    /// Testa registro NSS pelo CLI nativo com perfil e carteira sintéticos.
    FirefoxNssSmoke {
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        client: PathBuf,
    },
    #[command(hide = true)]
    FirefoxWorker {
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        home: PathBuf,
    },
    /// Valida instalação RPM em usuário sintético de contêiner descartável.
    RpmRuntimeSmoke {
        #[arg(long)]
        check_preserved: Option<PathBuf>,
    },
    /// Testa bandeja em barramento D-Bus privado com host e GUI simulados.
    TraySmoke {
        #[arg(long)]
        binary: PathBuf,
    },
    #[command(hide = true)]
    TrayWorker {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        home: PathBuf,
    },
    /// Testa autenticação TLS 1.2 Chromium apenas em contêiner de laboratório.
    ChromiumTlsSmoke {
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        module: PathBuf,
        #[arg(long)]
        no_sandbox: bool,
    },
}

fn run(cli: Cli) -> Result<bool> {
    let root = cli
        .root
        .canonicalize()
        .context("Árvore de fontes inexistente")?;
    match cli.command {
        Action::CheckPublication => {
            let names = publication::candidates(&root)?;
            let issues = publication::inspect(&root, &names)?;
            publication::report(names.len(), &issues);
            return Ok(issues.is_empty());
        }
        Action::PackageSources { output } => package::sources(&root, &output)?,
        Action::PackageLinux { target, output } => {
            let output = output.unwrap_or(PathBuf::from(format!(
                "dist/axia-remotedid-{}-fedora44-x86_64.tar.gz",
                package::version(&root)?
            )));
            package::linux(&root, &target, &output)?;
        }
        Action::PackageRpm {
            target,
            output_dir,
            rpmbuild,
            container,
            container_image,
        } => {
            package::rpm(
                &root,
                &target,
                &output_dir,
                &rpmbuild,
                container,
                &container_image,
            )?;
        }
        Action::FedoraDiagnostic { module } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&diagnostic::collect(module.as_deref())?)?
            );
        }
        Action::GenerateVectors { output } => fixtures::generate(&output)?,
        Action::CliSmoke { binary, report } => cli_smoke::smoke(&root, &binary, &report)?,
        Action::NativeUiSmoke { target } => native_ui::smoke(&root, &target)?,
        Action::Pkcs11Inventory { module, report } => {
            let value = pkcs::inventory(&module)?;
            if let Some(report) = report {
                lab::report(&report, &value)?;
            }
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Action::SignPkcs11Test {
            certificate_index,
            inventory_only,
            module,
            helper,
            state_dir,
        } => {
            if inventory_only {
                let (module, state) = sign_test::inventory_paths(module, state_dir)?;
                // Run in a child: never modify the parent process environment.
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command
                    .args(["pkcs11-inventory", "--module"])
                    .arg(module)
                    .env("DESKTOPID_STATE_DIR", state);
                let output = lab::successful(&mut command)?;
                print!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                let (module, helper, state) = sign_test::paths(module, helper, state_dir)?;
                sign_test::run(&module, &helper, &state, certificate_index)?;
            }
        }
        Action::Pkcs11Smoke {
            module,
            client,
            report,
        } => pkcs_smoke::smoke(&root, &module, &client, &report)?,
        Action::Pkcs11Worker { module, home } => pkcs_smoke::worker(&root, &module, &home)?,
        Action::FirefoxNssSmoke { module, client } => {
            firefox_smoke::smoke(&root, &module, &client)?
        }
        Action::FirefoxWorker {
            module,
            client,
            home,
        } => firefox_smoke::worker(&module, &client, &home)?,
        Action::RpmRuntimeSmoke { check_preserved } => {
            rpm_smoke::smoke(&root, check_preserved.as_deref())?
        }
        Action::TraySmoke { binary } => tray_smoke::smoke(&binary)?,
        Action::TrayWorker { binary, home } => tray_smoke::worker(&binary, &home)?,
        Action::ChromiumTlsSmoke {
            client,
            module,
            no_sandbox,
        } => chromium_smoke::smoke(&root, &client, &module, no_sandbox)?,
    }
    Ok(true)
}

fn main() -> ExitCode {
    let result = if lab_helper::called_as_helper() {
        lab_helper::broker().map(|()| true)
    } else {
        match tray_smoke::gui_helper() {
            Ok(true) => Ok(true),
            Ok(false) => run(Cli::parse()),
            Err(error) => Err(error),
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Erro: {error:#}");
            ExitCode::FAILURE
        }
    }
}
