//! Persist an installation once; failures must not cause blind registration.
use crate::{
    client::{Api, DesktopInfo, Login},
    identity::Identity,
    storage::{Config, Store},
};
use anyhow::{Context, Result, ensure};

pub fn register_once(
    api: &Api,
    store: &Store,
    config: &mut Config,
    identity: &Identity,
    login: &Login,
    info: &DesktopInfo<'_>,
) -> Result<()> {
    register_with_mode(
        api,
        store,
        config,
        identity,
        login,
        info,
        RegistrationMode::First,
    )
}

pub fn register_retry_once(
    api: &Api,
    store: &Store,
    config: &mut Config,
    identity: &Identity,
    login: &Login,
    info: &DesktopInfo<'_>,
) -> Result<()> {
    register_with_mode(
        api,
        store,
        config,
        identity,
        login,
        info,
        RegistrationMode::DiagnosticRetry,
    )
}

pub fn register_domain_fix_once(
    api: &Api,
    store: &Store,
    config: &mut Config,
    identity: &Identity,
    login: &Login,
    info: &DesktopInfo<'_>,
) -> Result<()> {
    ensure!(
        info.domain == crate::client::DEFAULT_NETWORK_DOMAIN,
        "A tentativa corrigida exige domínio Local"
    );
    register_with_mode(
        api,
        store,
        config,
        identity,
        login,
        info,
        RegistrationMode::DomainFix,
    )
}
enum RegistrationMode {
    First,
    DiagnosticRetry,
    DomainFix,
}

fn register_with_mode(
    api: &Api,
    store: &Store,
    config: &mut Config,
    identity: &Identity,
    login: &Login,
    info: &DesktopInfo<'_>,
    mode: RegistrationMode,
) -> Result<()> {
    let (saved, _) = store.load()?;
    ensure!(
        saved.desktop_code.is_none() && config.desktop_code.is_none(),
        "Esta identidade já possui cadastro"
    );
    ensure!(
        saved.server == config.server
            && saved.access_url == config.access_url
            && saved.public_key_sha256 == config.public_key_sha256
            && identity.fingerprint()? == saved.public_key_sha256,
        "Identidade/configuração mudou durante o cadastro"
    );
    match mode {
        RegistrationMode::First => store.begin_registration()?,
        RegistrationMode::DiagnosticRetry => store.begin_registration_retry()?,
        RegistrationMode::DomainFix => store.begin_domain_fix_retry()?,
    }
    let code =
        match api.register(login, info, identity) {
            Ok(code) => code,
            Err(error) => {
                if let Some(service) = error.downcast_ref::<crate::client::ServiceError>() {
                    match store.save_registration_error(service.status, &service.body) {
                        Ok(path) => {
                            return Err(error.context(format!(
                                "Resposta de erro preservada no arquivo privado {}",
                                path.display()
                            )));
                        }
                        Err(_) => return Err(error.context(
                            "Não foi possível preservar a resposta localmente; marcadores mantidos",
                        )),
                    }
                }
                return Err(error);
            }
        };
    config.desktop_code = Some(code);
    store.save_registration(config).context(
        "Cadastro aceito, mas houve falha ao salvar. Não repetir; confira registration.json",
    )?;
    Ok(())
}
