use anyhow::{Context, Result, ensure};
use desktopid_client::{bridge::Cancelled, storage::Store};
use zeroize::Zeroizing;
fn dialog(label: &str, cached: bool) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    ensure!(
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some(),
        "Sessão gráfica indisponível; nenhuma autorização ou assinatura solicitada"
    );
    crate::ui::init()?;
    let text = if cached {
        format!("Certificado: {label}\nAutorização por até 10 minutos neste aplicativo.")
    } else {
        format!("Certificado: {label}\nAutorize o uso deste certificado para continuar.")
    };
    let fields = [
        crate::ui::Field {
            label: "Senha do aplicativo Certisign",
            secret: true,
            required: true,
        },
        crate::ui::Field {
            label: "Código temporário do Certisign",
            secret: true,
            required: true,
        },
    ];
    let Some(mut values) = crate::ui::form(
        "Axia RemotedID — Autorizar certificado",
        &text,
        &fields,
        "Autorizar",
        Some(std::time::Duration::from_secs(180)),
    ) else {
        return Err(Cancelled.into());
    };
    let otp = values.pop().context("OTP ausente")?;
    let password = values.pop().context("Senha ausente")?;
    Ok((password, otp))
}

pub fn run(store: &Store) -> Result<()> {
    desktopid_client::bridge::run_with(store, |label| dialog(label, false))
}
pub fn run_broker(store: &Store) -> Result<()> {
    desktopid_client::bridge::run_broker_with(store, |label| dialog(label, true))
}
