use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use desktopid_client::{
    client::{Api, DEFAULT_NETWORK_DOMAIN, DesktopInfo},
    protocol::{Algorithm, CertificateReference, reference_from_key_name, verify_digest},
    signing::{
        LAB_DOCUMENT, SigningAuthorization, recover_signing_test, sign_and_verify_recorded,
        validate_certificate,
    },
    storage::{Store, write_new_private},
    workflow::{register_domain_fix_once, register_once, register_retry_once},
};
use openssl::{
    hash::Hasher,
    x509::{X509, X509NameRef},
};
use serde_json::json;
use std::{
    fs,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "axia-remotedid",
    bin_name = "axia-remotedid",
    version,
    about = "Axia RemotedID — Certificado em nuvem para Linux",
    after_help = "Sem comando, abre a janela gráfica de conexão."
)]
struct Cli {
    /// Diretório Linux da identidade local (padrão: ~/.config/desktopid-linux).
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Abre a janela de login, cadastro deste computador e consulta de certificados.
    Gui,
    /// Detecta os navegadores instalados e configura seus certificados.
    SetupBrowsers {
        /// Mostra apenas a detecção local, sem alterar configuração.
        #[arg(long, conflicts_with = "yes")]
        list: bool,
        /// Configuração explícita pelo terminal, sem janela de confirmação.
        #[arg(long)]
        yes: bool,
        #[arg(long, value_enum, requires = "yes")]
        family: Option<axia_desktop::browsers::Family>,
    },
    /// Compatibilidade: configura um perfil específico do Firefox.
    SetupFirefox {
        #[arg(long)]
        profile: Option<PathBuf>,
        #[arg(long)]
        module: Option<PathBuf>,
        #[arg(long)]
        home: Option<PathBuf>,
        /// Configuração local explícita, com resultado JSON, sem rede.
        #[arg(long)]
        yes: bool,
    },
    /// Instala a build compilada para este usuário, sem modificar sua conta.
    InstallUser {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Ativa/desativa a abertura com a sessão; sem opções, mostra a configuração.
    Autostart {
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
    },
    #[command(hide = true)]
    Startup,
    /// Auxiliar interno PKCS11: pedido público por stdin, autorização gráfica local.
    #[command(hide = true)]
    Pkcs11Sign,
    /// Auxiliar interno persistente: sessão em memória por dez minutos.
    #[command(hide = true)]
    Pkcs11Broker,
    /// Verifica uma assinatura candidata salva, sem rede nem nova assinatura.
    RecoverSignTest {
        #[arg(long)]
        run_id: String,
    },
    /// Escolhe um certificado salvo e assina um texto de laboratório com SHA-256.
    SignTest {
        /// Número mostrado na lista (começa em 1); se omitido, pergunta no terminal.
        #[arg(long, conflicts_with = "list")]
        certificate_index: Option<usize>,
        /// Mostra somente tipo, validade e impressão digital, sem rede nem assinatura.
        #[arg(long, conflicts_with = "push")]
        list: bool,
        #[arg(long)]
        push: bool,
    },
    /// Cria ou reutiliza identidade Linux, cadastra uma vez e consulta a carteira.
    Connect {
        #[arg(long)]
        email: Option<String>,
        #[arg(long, default_value = "https://remoteidcertisign.com.br")]
        server: String,
        #[arg(long, default_value = "")]
        access_url: String,
        #[arg(long, default_value = "Axia RemotedID Fedora Linux")]
        desktop_name: String,
        /// Uma repetição diagnóstica explícita, com a mesma chave e histórico preservado.
        #[arg(long, conflicts_with = "retry_domain_fix")]
        retry_registration: bool,
        /// Uma tentativa com dominioRede=Local, corrigido conforme o original.
        #[arg(long, conflicts_with = "retry_registration")]
        retry_domain_fix: bool,
    },
    /// Testa somente o login; não cria identidade, cadastro ou assinatura.
    LoginCheck {
        /// Se omitido, o e-mail é solicitado no terminal local.
        #[arg(long)]
        email: Option<String>,
        /// Origem recuperada dos instaladores; aceitação da conta ainda em teste.
        #[arg(long, default_value = "https://remoteidcertisign.com.br")]
        server: String,
        #[arg(long, default_value = "")]
        access_url: String,
        /// Mostra apenas metadados estruturais do token, sem token nem IDs.
        #[arg(long)]
        diagnostics: bool,
    },
    /// Examina a identidade e a tentativa de cadastro somente no disco, sem rede.
    RegistrationDiagnostic,
    /// Compara a autenticação em consultas do próprio perfil, sem cadastrar.
    AuthorizationDiagnostic {
        #[arg(long)]
        email: Option<String>,
        #[arg(long, default_value = "https://remoteidcertisign.com.br")]
        server: String,
        #[arg(long, default_value = "")]
        access_url: String,
    },
    /// Gera somente a identidade RSA local; não acessa a rede.
    Init {
        /// Origem HTTPS confirmada para esta conta, sem /api.
        #[arg(long)]
        server: String,
        /// Valor urlAcesso usado pela organização da conta.
        #[arg(long)]
        access_url: String,
    },
    /// Mostra configuração sem chave privada, senhas ou tokens.
    Status,
    /// Faz login e cadastra uma nova instalação Linux (uma tentativa).
    Register {
        #[arg(long)]
        email: String,
        #[arg(long)]
        desktop_name: String,
        #[arg(long)]
        local_user: String,
        #[arg(long, default_value = "")]
        domain: String,
    },
    /// Consulta certificados públicos e opcionalmente exporta um PEM.
    Wallet {
        #[arg(long)]
        export_index: Option<usize>,
        #[arg(long, requires = "export_index")]
        output: Option<PathBuf>,
    },
    /// Assina o hash de um arquivo e verifica antes de salvar a assinatura RSA.
    Sign {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        certificate: PathBuf,
        /// keyName exatamente como a carteira o devolveu. Série e emissor do
        /// protocolo são derivados dele, separados por ';'.
        #[arg(long, conflicts_with_all = ["issue", "serial"])]
        key_name: Option<String>,
        /// Emissor literal, para sobrepor a derivação do keyName.
        #[arg(long, requires = "serial")]
        issue: Option<String>,
        /// Série literal, para sobrepor a derivação do keyName.
        #[arg(long, requires = "issue")]
        serial: Option<String>,
        #[arg(long, value_enum, default_value = "sha256")]
        algorithm: Algorithm,
        #[arg(long)]
        push: bool,
        #[arg(long)]
        output: PathBuf,
    },
    /// Verifica uma assinatura RSA pré-hash local, sem rede.
    Verify {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        certificate: PathBuf,
        #[arg(long)]
        signature: PathBuf,
        #[arg(long, value_enum, default_value = "sha256")]
        algorithm: Algorithm,
    },
}

fn default_state_dir() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(root).join("desktopid-linux"));
    }
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME Linux não definido")?)
            .join(".config/desktopid-linux"),
    )
}

fn name_text(name: &X509NameRef) -> String {
    name.entries()
        .map(|entry| {
            format!(
                "{}={}",
                entry.object().nid().short_name().unwrap_or("OID"),
                entry
                    .data()
                    .to_string()
                    .unwrap_or_else(|_| "<não UTF-8>".into())
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn certificate(path: &PathBuf) -> Result<X509> {
    let bytes = fs::read(path)?;
    X509::from_pem(&bytes)
        .or_else(|_| X509::from_der(&bytes))
        .context("Certificado PEM/DER inválido")
}

fn file_digest(path: &PathBuf, algorithm: Algorithm) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Hasher::new(algorithm.message_digest())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count])?;
    }
    Ok(hasher.finish()?.to_vec())
}

fn secret(prompt: &str) -> Result<Zeroizing<String>> {
    ensure!(
        io::stdin().is_terminal(),
        "Execute em terminal interativo para informar os dados de autenticação"
    );
    Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
}

fn prompt_email() -> Result<String> {
    ensure!(
        io::stdin().is_terminal(),
        "Execute em terminal interativo para informar seu e-mail"
    );
    print!("E-mail usado no aplicativo Certisign: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_owned();
    ensure!(
        !value.is_empty() && value.contains('@'),
        "Informe um e-mail válido"
    );
    Ok(value)
}

fn run(cli: Cli) -> Result<()> {
    let command = cli.command.unwrap_or(Command::Gui);
    if let Command::AuthorizationDiagnostic {
        email,
        server,
        access_url,
    } = &command
    {
        let api = Api::new(server)?;
        println!(
            "Diagnóstico de autenticação: login e duas consultas do próprio perfil. Nenhum cadastro será solicitado."
        );
        let email = match email {
            Some(email) => email.clone(),
            None => prompt_email()?,
        };
        let password = secret("Senha de acesso do aplicativo Certisign: ")?;
        let login = api.login(&email, access_url, &password)?;
        let results = [
            api.authorization_diagnostic(&login, false)?,
            api.authorization_diagnostic(&login, true)?,
        ];
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"token_structure":login.diagnostics(),
            "profile_checks":results,"registration_requested":false,"signature_requested":false}))?
        );
        return Ok(());
    }
    // Login preflight is independent of the installation key and never creates
    // state. No registration/session/signature endpoint is called here.
    if let Command::LoginCheck {
        email,
        server,
        access_url,
        diagnostics,
    } = &command
    {
        let api = Api::new(server)?;
        println!("Teste de login pelo protocolo recuperado, servidor: {server}");
        let email = match email {
            Some(email) => email.clone(),
            None => prompt_email()?,
        };
        let password = secret("Senha de acesso do aplicativo Certisign: ")?;
        let login = api.login(&email, access_url, &password)?;
        if *diagnostics {
            println!("{}", serde_json::to_string_pretty(&login.diagnostics())?);
        }
        println!(
            "Login aceito com os campos esperados. Nenhum computador foi cadastrado e nenhuma assinatura foi solicitada."
        );
        return Ok(());
    }
    // Offline verify does not need or create an installation directory.
    if let Command::Verify {
        file,
        certificate: cert,
        signature,
        algorithm,
    } = &command
    {
        let public = certificate(cert)?.public_key()?;
        verify_digest(
            &public,
            *algorithm,
            &file_digest(file, *algorithm)?,
            &fs::read(signature)?,
        )?;
        println!("Assinatura RSA verificada para o arquivo e certificado informados.");
        return Ok(());
    }
    let root = match cli.state_dir {
        Some(path) => path,
        None => default_state_dir()?,
    };
    let store = Store::new(root)?;
    if let Command::InstallUser { target, home } = command {
        axia_desktop::desktop::user()?;
        let target = match target {
            Some(path) => path,
            None => std::env::current_exe()?
                .parent()
                .context("Pasta da build ausente")?
                .to_owned(),
        };
        let home = home.unwrap_or(axia_desktop::desktop::home()?);
        let entry = axia_desktop::desktop::install(&target, &home)?;
        println!(
            "Axia RemotedID instalado para este usuário. Atalho: {}",
            entry.display()
        );
        return Ok(());
    }
    if let Command::SetupBrowsers { list, yes, family } = command {
        let home = axia_desktop::desktop::home()?;
        let inventory = axia_desktop::browsers::discover(&home)?;
        if list {
            println!("{}", serde_json::to_string_pretty(&inventory)?);
            return Ok(());
        }
        if !yes {
            return axia_desktop::browsers::setup_ui(&store, None);
        }
        let module = axia_desktop::browsers::module(&home)?;
        let targets: Vec<_> = inventory
            .targets
            .iter()
            .filter(|target| family.is_none_or(|family| family == target.family))
            .collect();
        ensure!(
            !targets.is_empty(),
            "Nenhum navegador nativo disponível para configurar"
        );
        let mut results = Vec::new();
        for target in targets {
            results.push(axia_desktop::browsers::register(
                target, &module, &home, true,
            )?);
        }
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }
    if let Command::SetupFirefox {
        profile,
        module,
        home,
        yes,
    } = command
    {
        if !yes {
            return axia_desktop::browsers::setup_ui(&store, profile);
        }
        let home = home.unwrap_or(axia_desktop::desktop::home()?);
        let module = match module {
            Some(path) => path,
            None => axia_desktop::browsers::module(&home)?,
        };
        let target = match profile {
            Some(database) => axia_desktop::browsers::Target {
                family: axia_desktop::browsers::Family::Firefox,
                label: "Firefox".into(),
                database,
                create: false,
            },
            None => {
                let targets: Vec<_> = axia_desktop::browsers::discover(&home)?
                    .targets
                    .into_iter()
                    .filter(|target| target.family == axia_desktop::browsers::Family::Firefox)
                    .collect();
                ensure!(
                    targets.len() == 1,
                    "Escolha um perfil Firefox explicitamente"
                );
                targets.into_iter().next().context("Perfil ausente")?
            }
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&axia_desktop::browsers::register(
                &target, &module, &home, true
            )?)?
        );
        return Ok(());
    }
    if let Command::Autostart { enable, disable } = command {
        let root = desktopid_client::autostart::config_root()?;
        if enable || disable {
            desktopid_client::autostart::configure(
                &root,
                &desktopid_client::autostart::executable()?,
                store.path(),
                enable,
            )?;
        }
        println!(
            "Inicialização automática do Axia RemotedID: {}",
            if desktopid_client::autostart::enabled(&root)? {
                "ativada"
            } else {
                "desativada"
            }
        );
        return Ok(());
    }
    if let Command::Startup = command {
        return axia_desktop::gui::run_startup(&store);
    }
    if let Command::Gui = command {
        return axia_desktop::gui::run(&store);
    }
    if let Command::Pkcs11Sign = command {
        return axia_desktop::bridge::run(&store);
    }
    if let Command::Pkcs11Broker = command {
        return axia_desktop::bridge::run_broker(&store);
    }
    if let Command::RecoverSignTest { run_id } = &command {
        let directory = recover_signing_test(&store, run_id)?;
        println!(
            "Assinatura recuperada e verificada localmente. Nenhuma operação de rede foi realizada."
        );
        println!("Artefatos privados em {}", directory.display());
        return Ok(());
    }
    if let Command::RegistrationDiagnostic = command {
        let (config, identity) = store.load()?;
        let public = identity.public_pem()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "scope":"local-registration-diagnostic-no-network",
                "server":config.server,"access_url_empty":config.access_url.is_empty(),
                "identity_key_matches_config":identity.fingerprint()? == config.public_key_sha256,
                "public_key_format":"PEM SPKI","public_key_rsa_bits":2048,
                "public_key_lf_only":!public.contains('\r'),
                "registration_attempt_present":store.path().join("registration-attempt.json").try_exists()?,
                "registration_journal_present":store.path().join("registration.json").try_exists()?,
                "desktop_code_saved":config.desktop_code.is_some(),
                "wallet_saved":store.path().join("wallet.json").try_exists()?,
                "registration_os_field":"Linux","registration_bearer_style":"legacy-no-space"
            }))?
        );
        return Ok(());
    }
    if let Command::Connect {
        email,
        server,
        access_url,
        desktop_name,
        retry_registration,
        retry_domain_fix,
    } = &command
    {
        let api = Api::new(server)?;
        // Guard before creating an identity or reading credentials: this command
        // is an interactive operation, never part of the automated test suite.
        ensure!(
            io::stdin().is_terminal(),
            "Execute connect no seu terminal interativo"
        );
        ensure!(
            !(*retry_registration || *retry_domain_fix) || store.path().try_exists()?,
            "Repetição exige a identidade existente e uma tentativa anterior"
        );
        let (mut config, identity) = if store.path().try_exists()? {
            store.load()?
        } else {
            store.initialize(server, access_url)?;
            println!("Identidade Linux criada em {}", store.path().display());
            store.load()?
        };
        ensure!(
            config.server == *server && config.access_url == *access_url,
            "A identidade existente usa outro servidor/acesso. Ela foi preservada; confira status"
        );
        if config.desktop_code.is_none() {
            if *retry_domain_fix {
                store.ensure_domain_fix_retry_available()?;
            } else if *retry_registration {
                store.ensure_registration_retry_available()?;
            } else {
                store.ensure_registration_available()?;
            }
            let email = match email {
                Some(email) => email.clone(),
                None => prompt_email()?,
            };
            let password = secret("Senha de acesso do aplicativo Certisign: ")?;
            let login = api.login(&email, &config.access_url, &password)?;
            if *retry_domain_fix {
                println!(
                    "Login aceito. Uma tentativa com a mesma identidade e domínio Local, conforme o original."
                );
            } else if *retry_registration {
                println!(
                    "Login aceito. Repetição diagnóstica única com a mesma identidade e os mesmos campos de cadastro."
                );
            } else {
                println!("Login aceito. Cadastrando esta identidade Linux...");
            }
            let user = std::env::var("USER").unwrap_or_else(|_| "linux".into());
            let register = if *retry_domain_fix {
                register_domain_fix_once
            } else if *retry_registration {
                register_retry_once
            } else {
                register_once
            };
            register(
                &api,
                &store,
                &mut config,
                &identity,
                &login,
                &DesktopInfo {
                    name: desktop_name,
                    local_user: &user,
                    domain: DEFAULT_NETWORK_DOMAIN,
                },
            )?;
            println!("Instalação Linux cadastrada e código salvo localmente.");
        } else {
            println!("Reutilizando a instalação Linux já cadastrada.");
        }
        let desktop = config
            .desktop_code
            .as_deref()
            .context("Código da instalação ausente")?;
        let Some(wallet) = api.wallet(desktop, &identity)? else {
            println!(
                "Cadastro preservado. Carteira respondeu HTTP 304; consulte novamente quando houver alteração."
            );
            return Ok(());
        };
        store.save_wallet(&wallet)?;
        let compatible = wallet
            .iter()
            .filter(|entry| reference_from_key_name(&entry.key_name).is_ok())
            .count();
        println!(
            "Carteira consultada: {} certificado(s); {} com referência de assinatura reconhecida.",
            wallet.len(),
            compatible
        );
        println!(
            "Certificados públicos e referências salvos em {}",
            store.path().join("wallet.json").display()
        );
        println!("Nenhuma assinatura foi solicitada.");
        return Ok(());
    }
    if let Command::Init { server, access_url } = &command {
        let config = store.initialize(server, access_url)?;
        println!(
            "Identidade local criada em {}\nSHA256 da chave pública: {}\nNenhuma instalação foi cadastrada no serviço.",
            store.path().display(),
            config.public_key_sha256
        );
        return Ok(());
    }
    let (mut config, identity) = store.load()?;
    if let Command::SignTest {
        certificate_index,
        list,
        push,
    } = command
    {
        let wallet = store.load_wallet()?;
        ensure!(!wallet.is_empty(), "Carteira vazia; atualize com connect");
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        for (index, entry) in wallet.iter().enumerate() {
            let key = entry.certificate.public_key()?;
            let kind = entry
                .certificate
                .subject_name()
                .entries()
                .filter_map(|e| e.data().to_string().ok())
                .collect::<Vec<_>>()
                .join(" ")
                .to_uppercase();
            let kind = if kind.contains("E-CPF") {
                "e-CPF"
            } else if kind.contains("E-CNPJ") {
                "e-CNPJ"
            } else {
                "certificado"
            };
            let fingerprint = entry
                .certificate
                .digest(openssl::hash::MessageDigest::sha256())?;
            let short: String = fingerprint[..6]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            println!(
                "{}: {} | {} {} bits | vence {} | SHA256 {} | {}",
                index + 1,
                kind,
                if key.rsa().is_ok() {
                    "RSA"
                } else {
                    "outra chave"
                },
                key.bits(),
                entry.certificate.not_after(),
                short,
                if validate_certificate(entry, now).is_ok() {
                    "apto ao teste"
                } else {
                    "indisponível para este teste"
                }
            );
        }
        if list {
            return Ok(());
        }
        ensure!(
            io::stdin().is_terminal(),
            "Execute sign-test no seu terminal interativo"
        );
        let selected = if let Some(index) = certificate_index {
            index
        } else {
            print!("Número do certificado para o teste: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input
                .trim()
                .parse::<usize>()
                .context("Informe o número mostrado na lista")?
        };
        let entry = wallet
            .get(selected.checked_sub(1).context("A numeração começa em 1")?)
            .context("Número de certificado não existe")?;
        validate_certificate(entry, now)?;
        let desktop = config
            .desktop_code
            .as_deref()
            .context("Instalação ainda não cadastrada")?;
        let api = Api::new(&config.server)?;
        let directory = store.new_signing_test_dir()?;
        write_new_private(&directory.join("document.txt"), LAB_DOCUMENT)?;
        write_new_private(
            &directory.join("certificate.pem"),
            &entry.certificate.to_pem()?,
        )?;
        println!(
            "Será assinado somente o texto de laboratório salvo em {}.",
            directory.join("document.txt").display()
        );
        let pin = secret("Senha para autorizar o certificado (aplicativo Certisign): ")?;
        ensure!(
            !pin.is_empty(),
            "Senha vazia; nenhuma assinatura solicitada"
        );
        let otp = if push {
            Zeroizing::new(String::new())
        } else {
            secret("Código do celular (OTP): ")?
        };
        ensure!(
            push || !otp.is_empty(),
            "OTP vazio; nenhuma assinatura solicitada"
        );
        write_new_private(
            &directory.join("attempt.json"),
            &serde_json::to_vec_pretty(
                &json!({"operation":"laboratory-signature", "algorithm":"SHA256",
                "certificate_number":selected, "result":"requires-review"}),
            )?,
        )?;
        println!("Solicitando autorização e uma assinatura SHA-256; sem repetição automática.");
        let signature = sign_and_verify_recorded(
            &api,
            &identity,
            desktop,
            entry,
            LAB_DOCUMENT,
            &SigningAuthorization {
                password: &pin,
                otp: &otp,
                push,
            },
            Some(&directory),
        )
        .with_context(|| {
            format!(
                "Teste interrompido; artefatos preservados em {}",
                directory.display()
            )
        })?;
        write_new_private(&directory.join("signature.bin"), &signature)?;
        write_new_private(
            &directory.join("result.json"),
            &serde_json::to_vec_pretty(
                &json!({"operation":"laboratory-signature", "algorithm":"SHA256",
                "certificate_number":selected, "signature_verified":true,
                "verification_scope":"RSA signature matches laboratory document and selected certificate; no PKI trust validation"}),
            )?,
        )?;
        println!("Assinatura de teste aceita e verificada criptograficamente no Linux.");
        println!("Artefatos privados salvos em {}", directory.display());
        return Ok(());
    }
    if let Command::Status = command {
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }
    let api = Api::new(&config.server)?;
    match command {
        Command::Register {
            email,
            desktop_name,
            local_user,
            domain,
        } => {
            store.ensure_registration_available()?;
            let password = secret("Senha da conta RemoteID: ")?;
            let login = api.login(&email, &config.access_url, &password)?;
            register_once(
                &api,
                &store,
                &mut config,
                &identity,
                &login,
                &DesktopInfo {
                    name: &desktop_name,
                    local_user: &local_user,
                    domain: &domain,
                },
            )?;
            println!("Instalação cadastrada e código salvo localmente.");
        }
        Command::Wallet {
            export_index,
            output,
        } => {
            let desktop = config
                .desktop_code
                .as_deref()
                .context("Instalação ainda não cadastrada")?;
            let Some(wallet) = api.wallet(desktop, &identity)? else {
                println!("Serviço retornou HTTP 304; não há carteira em cache nesta versão.");
                return Ok(());
            };
            let summary: Vec<_> = wallet.iter().enumerate().map(|(index, entry)| {
                // Derived from keyName, as the original provider does; the
                // *_display fields come from the X.509 and are not the protocol
                // values. A keyName that does not split is reported, not fixed.
                let (serial, issue) = match reference_from_key_name(&entry.key_name) {
                    Ok((serial, issue)) => (json!(serial), json!(issue)),
                    Err(error) => (json!(null), json!(format!("{error:#}"))),
                };
                Ok(json!({
                "index": index, "keyName": entry.key_name, "subject_display": name_text(entry.certificate.subject_name()),
                "issuer_display": name_text(entry.certificate.issuer_name()),
                "serial_hex_display": entry.certificate.serial_number().to_bn()?.to_hex_str()?.to_string(),
                "not_after": entry.certificate.not_after().to_string(),
                "protocol_serialNumber": serial, "protocol_issue": issue,
            }))}).collect::<Result<_>>()?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            if let Some(index) = export_index {
                let entry = wallet
                    .get(index)
                    .context("Índice de certificado não existe")?;
                write_new_private(
                    &output.context("Informe --output para exportar")?,
                    &entry.certificate.to_pem()?,
                )?;
                println!(
                    "Certificado público exportado. Para assinar, informe --key-name com o keyName acima; os campos *_display vêm do X.509 e não são os valores do protocolo."
                );
            }
        }
        Command::Sign {
            file,
            certificate: cert,
            key_name,
            issue,
            serial,
            algorithm,
            push,
            output,
        } => {
            let (serial, issue) = match (key_name, issue, serial) {
                (Some(key_name), _, _) => reference_from_key_name(&key_name)?,
                (None, Some(issue), Some(serial)) => (serial, issue),
                _ => anyhow::bail!(
                    "Informe --key-name da carteira, ou --issue e --serial literais confirmados"
                ),
            };
            ensure!(
                !output.exists(),
                "Destino já existe; não assinar antes de escolher outro caminho"
            );
            let desktop = config
                .desktop_code
                .as_deref()
                .context("Instalação ainda não cadastrada")?;
            let cert = certificate(&cert)?;
            let public = cert.public_key()?;
            ensure!(
                public.rsa().is_ok(),
                "Esta versão suporta somente certificados RSA"
            );
            let digest = file_digest(&file, algorithm)?;
            let pin = secret("Senha para autorizar o certificado: ")?;
            let otp = if push {
                Zeroizing::new(String::new())
            } else {
                secret("Código do celular (OTP): ")?
            };
            let reference = CertificateReference {
                issue: &issue,
                serial: &serial,
            };
            let token = api.open_session(desktop, &reference, &pin, &otp, push, &identity)?;
            let signature =
                api.sign_digest(desktop, &reference, &token, algorithm, &digest, &identity)?;
            verify_digest(&public, algorithm, &digest, &signature)?;
            write_new_private(&output, &signature)?;
            println!(
                "Assinatura RSA verificada e salva em {}. O arquivo original não foi alterado.",
                output.display()
            );
        }
        Command::Init { .. }
        | Command::Status
        | Command::Verify { .. }
        | Command::LoginCheck { .. }
        | Command::RegistrationDiagnostic
        | Command::AuthorizationDiagnostic { .. }
        | Command::Connect { .. } => unreachable!(),
        Command::SignTest { .. } => unreachable!(),
        Command::RecoverSignTest { .. } => unreachable!(),
        Command::Pkcs11Sign
        | Command::Pkcs11Broker
        | Command::Gui
        | Command::Startup
        | Command::Autostart { .. } => unreachable!(),
        Command::SetupBrowsers { .. }
        | Command::SetupFirefox { .. }
        | Command::InstallUser { .. } => unreachable!(),
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let graphical = cli.command.as_ref().is_none_or(|command| {
        matches!(
            command,
            Command::Gui
                | Command::SetupBrowsers {
                    list: false,
                    yes: false,
                    ..
                }
                | Command::SetupFirefox { yes: false, .. }
        )
    });
    if let Err(error) = run(cli) {
        if graphical && gtk::is_initialized_main_thread() {
            axia_desktop::ui::message(&format!(
                "Não foi possível concluir a operação.\n\n{error:#}"
            ));
        }
        eprintln!("Erro: {error:#}");
        if error
            .downcast_ref::<desktopid_client::bridge::Cancelled>()
            .is_some()
        {
            std::process::exit(2);
        }
        std::process::exit(1);
    }
}
