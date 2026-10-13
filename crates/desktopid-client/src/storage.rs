//! Linux-only permissions. Keep identities on the Linux filesystem, not /mnt/c.
use crate::{client::Api, identity::Identity};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub server: String,
    pub access_url: String,
    pub public_key_sha256: String,
    pub desktop_code: Option<String>,
}

pub struct Store {
    root: PathBuf,
}

pub(crate) fn check_owned_private(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).context("Arquivo da identidade local ausente")?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Link simbólico recusado na identidade local"
    );
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "Tipo de arquivo inesperado"
    );
    // SAFETY: geteuid has no parameters or memory access requirements.
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() },
        "Identidade pertence a outro usuário Linux"
    );
    ensure!(
        metadata.mode() & 0o077 == 0,
        "Permissões locais devem ser 0700/0600"
    );
    Ok(())
}

pub fn write_new_private(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context("Destino já existe ou não pode ser criado")?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self> {
        ensure!(
            root.is_absolute(),
            "Diretório da identidade deve ser absoluto"
        );
        let resolved = if root.exists() {
            root.canonicalize()?
        } else {
            // The parent can be created; the identity directory itself must be new.
            let parent = root.parent().context("Diretório inválido")?;
            fs::create_dir_all(parent)?;
            parent
                .canonicalize()?
                .join(root.file_name().context("Diretório inválido")?)
        };
        ensure!(
            !resolved.starts_with("/mnt"),
            "Guarde a identidade no sistema de arquivos Linux, fora de /mnt"
        );
        Ok(Self { root })
    }
    pub fn path(&self) -> &Path {
        &self.root
    }
    pub fn initialize(&self, server: &str, access_url: &str) -> Result<Config> {
        Api::validate_base(server)?;
        let identity = Identity::generate()?;
        fs::create_dir(&self.root)
            .context("Diretório já existe; identidade não será substituída")?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        let config = Config {
            schema_version: 1,
            server: server.to_owned(),
            access_url: access_url.to_owned(),
            public_key_sha256: identity.fingerprint()?,
            desktop_code: None,
        };
        write_new_private(&self.root.join("identity.pem"), &identity.private_pem()?)?;
        write_new_private(
            &self.root.join("public.pem"),
            identity.public_pem()?.as_bytes(),
        )?;
        write_new_private(
            &self.root.join("config.json"),
            &serde_json::to_vec_pretty(&config)?,
        )?;
        Ok(config)
    }
    pub fn load(&self) -> Result<(Config, Identity)> {
        check_owned_private(&self.root, true)?;
        let config_path = self.root.join("config.json");
        let key_path = self.root.join("identity.pem");
        check_owned_private(&config_path, false)?;
        check_owned_private(&key_path, false)?;
        let config: Config = serde_json::from_slice(&fs::read(config_path)?)?;
        ensure!(
            config.schema_version == 1,
            "Versão de configuração incompatível"
        );
        let identity = Identity::from_pem(&Zeroizing::new(fs::read(key_path)?))?;
        ensure!(
            identity.fingerprint()? == config.public_key_sha256,
            "Chave não corresponde à configuração"
        );
        Ok((config, identity))
    }
    pub fn save_registration(&self, config: &Config) -> Result<()> {
        check_owned_private(&self.root, true)?;
        check_owned_private(&self.root.join("config.json"), false)?;
        // A failed remote registration is never retried automatically. Preserve
        // its returned code in a separate journal before replacing config.
        let journal = self.root.join("registration.json");
        write_new_private(&journal, &serde_json::to_vec_pretty(config)?)?;
        let pending = self.root.join("config.pending");
        write_new_private(&pending, &serde_json::to_vec_pretty(config)?)?;
        fs::rename(pending, self.root.join("config.json"))?;
        Ok(())
    }
    pub fn ensure_registration_available(&self) -> Result<()> {
        let (config, _) = self.load()?;
        ensure!(
            config.desktop_code.is_none(),
            "Esta identidade já possui cadastro"
        );
        ensure!(
            !self.root.join("registration.json").try_exists()?,
            "Há um código no journal; recuperar a configuração antes de cadastrar"
        );
        ensure!(
            !self.root.join("registration-attempt.json").try_exists()?,
            "Há uma tentativa de cadastro anterior; conferir o resultado antes de tentar novamente"
        );
        Ok(())
    }
    pub fn begin_registration(&self) -> Result<()> {
        self.ensure_registration_available()?;
        write_new_private(
            &self.root.join("registration-attempt.json"),
            b"{\"operation\":\"register\",\"result\":\"requires-review\"}\n",
        )
    }
    pub fn ensure_registration_retry_available(&self) -> Result<()> {
        let (config, _) = self.load()?;
        ensure!(
            config.desktop_code.is_none(),
            "A instalação já possui código; não repetir cadastro"
        );
        ensure!(
            !self.root.join("registration.json").try_exists()?,
            "Há um código no journal; recuperar antes de repetir cadastro"
        );
        ensure!(
            !self
                .root
                .join("registration-attempt.before-retry.json")
                .try_exists()?,
            "A repetição diagnóstica já foi utilizada; analisar seu resultado antes de outra tentativa"
        );
        let marker = self.root.join("registration-attempt.json");
        check_owned_private(&marker, false)?;
        let value: serde_json::Value = serde_json::from_slice(&fs::read(marker)?)?;
        ensure!(
            value.get("operation").and_then(serde_json::Value::as_str) == Some("register"),
            "Marcador de tentativa com formato inesperado"
        );
        Ok(())
    }
    /// One explicit diagnostic retry, preserving the old marker and the RSA key.
    /// The normal registration guard remains closed before and after this call.
    pub fn begin_registration_retry(&self) -> Result<()> {
        self.ensure_registration_retry_available()?;
        let (config, _) = self.load()?;
        let marker = self.root.join("registration-attempt.json");
        write_new_private(
            &self.root.join("registration-attempt.before-retry.json"),
            &fs::read(&marker)?,
        )?;
        let updated = serde_json::json!({"operation":"register","result":"requires-review",
            "attempt_kind":"explicit-diagnostic-retry","public_key_sha256":config.public_key_sha256});
        let pending = self.root.join("registration-attempt.pending");
        write_new_private(&pending, &serde_json::to_vec_pretty(&updated)?)?;
        fs::rename(pending, marker)?;
        Ok(())
    }
    pub fn ensure_domain_fix_retry_available(&self) -> Result<()> {
        let (config, _) = self.load()?;
        ensure!(
            config.desktop_code.is_none() && !self.root.join("registration.json").try_exists()?,
            "Há um cadastro aceito para recuperar; não repetir"
        );
        ensure!(
            !self
                .root
                .join("registration-attempt.before-domain-fix.json")
                .try_exists()?,
            "A tentativa com domínio corrigido já foi utilizada; analisar o resultado"
        );
        check_owned_private(
            &self.root.join("registration-attempt.before-retry.json"),
            false,
        )?;
        let active = self.root.join("registration-attempt.json");
        check_owned_private(&active, false)?;
        let value: serde_json::Value = serde_json::from_slice(&fs::read(active)?)?;
        ensure!(
            value["attempt_kind"] == "explicit-diagnostic-retry"
                && value["public_key_sha256"] == config.public_key_sha256,
            "O histórico não corresponde à tentativa diagnóstica anterior desta chave"
        );
        Ok(())
    }
    pub fn begin_domain_fix_retry(&self) -> Result<()> {
        self.ensure_domain_fix_retry_available()?;
        let (config, _) = self.load()?;
        let active = self.root.join("registration-attempt.json");
        write_new_private(
            &self
                .root
                .join("registration-attempt.before-domain-fix.json"),
            &fs::read(&active)?,
        )?;
        let updated = serde_json::json!({"operation":"register","result":"requires-review",
            "attempt_kind":"explicit-domain-default-fix","public_key_sha256":config.public_key_sha256,
            "payload_revision":2,"network_domain":crate::client::DEFAULT_NETWORK_DOMAIN});
        let pending = self.root.join("registration-attempt.pending");
        write_new_private(&pending, &serde_json::to_vec_pretty(&updated)?)?;
        fs::rename(pending, active)?;
        Ok(())
    }
    /// Raw response may contain personal data or echoed tokens: store privately,
    /// outside the repo, and never include it in a shareable report.
    pub(crate) fn save_registration_error(&self, status: u16, body: &[u8]) -> Result<PathBuf> {
        check_owned_private(&self.root, true)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = self
            .root
            .join(format!("registration-error-{timestamp}.json"));
        use base64::{Engine, engine::general_purpose::STANDARD};
        let encoded = STANDARD.encode(body);
        let value = serde_json::json!({"http_status":status,"scope":"private-registration-error-response",
            "body_base64":encoded});
        write_new_private(&path, &serde_json::to_vec_pretty(&value)?)?;
        Ok(path)
    }
    /// Read the wallet only when it belongs to the current identity/installation.
    pub fn load_wallet(&self) -> Result<Vec<crate::client::WalletCertificate>> {
        let (config, _) = self.load()?;
        self.wallet_for_config(&config)
    }

    /// Inventory in a host application needs only the installation PUBLIC key.
    /// Never load identity.pem into Firefox/Java merely to enumerate certificates.
    pub fn load_wallet_public(&self) -> Result<Vec<crate::client::WalletCertificate>> {
        check_owned_private(&self.root, true)?;
        let config_path = self.root.join("config.json");
        let public_path = self.root.join("public.pem");
        check_owned_private(&config_path, false)?;
        check_owned_private(&public_path, false)?;
        let config: Config = serde_json::from_slice(&fs::read(config_path)?)?;
        ensure!(
            config.schema_version == 1,
            "Versão de configuração incompatível"
        );
        let public = openssl::pkey::PKey::public_key_from_pem(&fs::read(public_path)?)?;
        let fingerprint: String = openssl::sha::sha256(&public.public_key_to_der()?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ensure!(
            fingerprint == config.public_key_sha256,
            "Chave pública não corresponde à configuração"
        );
        self.wallet_for_config(&config)
    }

    fn wallet_for_config(&self, config: &Config) -> Result<Vec<crate::client::WalletCertificate>> {
        ensure!(
            config.desktop_code.is_some(),
            "Instalação ainda não cadastrada"
        );
        let path = self.root.join("wallet.json");
        check_owned_private(&path, false)?;
        ensure!(
            fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
            "Carteira local excede o limite"
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(
            value["schema_version"] == 1
                && value["public_key_sha256"] == config.public_key_sha256
                && value["desktop_code"].as_str() == config.desktop_code.as_deref(),
            "Carteira não corresponde à identidade/instalação atual"
        );
        use base64::{Engine, engine::general_purpose::STANDARD};
        value["certificados"]
            .as_array()
            .context("Carteira sem certificados")?
            .iter()
            .enumerate()
            .map(|(index, item)| {
                ensure!(
                    item["index"].as_u64() == Some(index as u64),
                    "Índice da carteira inconsistente"
                );
                let key_name = item["keyName"]
                    .as_str()
                    .context("Referência ausente")?
                    .to_owned();
                let der = STANDARD
                    .decode(item["base64"].as_str().context("Certificado ausente")?)
                    .context("Base64 de certificado inválido")?;
                Ok(crate::client::WalletCertificate {
                    key_name,
                    certificate: openssl::x509::X509::from_der(&der)
                        .context("Certificado DER inválido")?,
                })
            })
            .collect()
    }

    /// Create a private directory for a single, explicitly requested laboratory run.
    pub fn new_signing_test_dir(&self) -> Result<PathBuf> {
        self.load()?;
        let parent = self.root.join("signing-tests");
        if !parent.try_exists()? {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&parent)?;
        }
        check_owned_private(&parent, true)?;
        let mut random = [0u8; 16];
        openssl::rand::rand_bytes(&mut random)?;
        let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory = parent.join(name);
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(directory)
    }

    pub(crate) fn existing_signing_test_dir(&self, run_id: &str) -> Result<PathBuf> {
        self.load()?;
        ensure!(
            run_id.len() == 32
                && run_id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Identificador deve conter 32 caracteres hexadecimais minúsculos"
        );
        let parent = self.root.join("signing-tests");
        check_owned_private(&parent, true)?;
        let directory = parent.join(run_id);
        check_owned_private(&directory, true)?;
        Ok(directory)
    }

    /// Save public wallet material and its original keyName; no private keys or tokens.
    pub fn save_wallet(&self, certificates: &[crate::client::WalletCertificate]) -> Result<()> {
        let (config, _) = self.load()?;
        ensure!(
            config.desktop_code.is_some(),
            "Carteira exige uma instalação cadastrada"
        );
        use base64::{Engine, engine::general_purpose::STANDARD};
        let items: Vec<_> = certificates
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                Ok(serde_json::json!({"index":index, "keyName":entry.key_name,
                "base64":STANDARD.encode(entry.certificate.to_der()?)}))
            })
            .collect::<Result<_>>()?;
        let wallet = serde_json::json!({"schema_version":1,"public_key_sha256":config.public_key_sha256,
            "desktop_code":config.desktop_code,"certificados":items});
        let destination = self.root.join("wallet.json");
        if destination.try_exists()? {
            check_owned_private(&destination, false)?;
        }
        let pending = self.root.join("wallet.pending");
        write_new_private(&pending, &serde_json::to_vec_pretty(&wallet)?)?;
        fs::rename(pending, destination)?;
        Ok(())
    }
}

pub(crate) fn read_private(path: &Path) -> Result<Vec<u8>> {
    check_owned_private(path, false)?;
    ensure!(
        fs::metadata(path)?.len() <= 4 * 1024 * 1024,
        "Artefato local excede o limite"
    );
    Ok(fs::read(path)?)
}
