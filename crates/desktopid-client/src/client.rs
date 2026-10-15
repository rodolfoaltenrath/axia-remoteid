use crate::{
    identity::Identity,
    protocol::{self, Algorithm, CertificateReference},
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::x509::X509;
use reqwest::{
    Url,
    blocking::Client,
    header::{AUTHORIZATION, HeaderValue},
    redirect::Policy,
};
use serde_json::{Value, json};
use std::{
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
/// macOS GUI InstallationModel::domainNameLeftBlank @0x10000d5e0 assigns L"Local".
pub const DEFAULT_NETWORK_DOMAIN: &str = "Local";

pub(crate) struct ServiceError {
    pub(crate) status: u16,
    pub(crate) body: Zeroizing<Vec<u8>>,
    summary: String,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Serviço respondeu HTTP {}; {}",
            self.status, self.summary
        )
    }
}
impl std::fmt::Debug for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for ServiceError {}
/// Expose only the HTTP status to native UI callers, never the service body.
pub fn service_status(error: &anyhow::Error) -> Option<u16> {
    error
        .downcast_ref::<ServiceError>()
        .map(|error| error.status)
}
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
pub struct Api {
    base: Url,
    http: Client,
}
pub struct Login {
    pub user_id: String,
    pub organization_id: String,
    token: Zeroizing<String>,
}
impl Login {
    /// Structural metadata only: never serialize the token or its JWT claims.
    pub fn diagnostics(&self) -> Value {
        json!({"token_length":self.token.len(),
            "token_has_leading_whitespace":self.token.starts_with(char::is_whitespace),
            "token_has_trailing_whitespace":self.token.ends_with(char::is_whitespace),
            "token_has_bearer_prefix":self.token.to_ascii_lowercase().starts_with("bearer"),
            "token_has_three_dot_segments":self.token.split('.').count()==3,
            "user_id_present":!self.user_id.is_empty(),
            "organization_id_present":!self.organization_id.is_empty()})
    }
}
pub struct WalletCertificate {
    pub key_name: String,
    pub certificate: X509,
}

pub struct DesktopInfo<'a> {
    pub name: &'a str,
    pub local_user: &'a str,
    pub domain: &'a str,
}

fn text_field(value: &Value, field: &str) -> Result<String> {
    let value = value.get(field).context(format!("Campo {field} ausente"))?;
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) if value.as_i64().is_some() => value.to_string(),
        _ => bail!("Campo {field} com tipo inesperado"),
    };
    ensure!(!text.is_empty(), "Campo {field} vazio");
    Ok(text)
}

/// Do not print arbitrary server messages: they may echo passwords, tokens or
/// personal data. Report only fixed field names and fixed diagnostic categories.
fn error_summary(bytes: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return "corpo não JSON; texto omitido".into();
    };
    let fields = [
        "message",
        "Message",
        "error",
        "errors",
        "status",
        "title",
        "detail",
        "ExceptionMessage",
        "ExceptionType",
        "StackTrace",
    ];
    let present: Vec<_> = fields
        .iter()
        .filter(|field| value.get(**field).is_some())
        .copied()
        .collect();
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    let categories: Vec<_> = [
        (
            "autorizacao",
            &[
                "authorization",
                "bearer",
                "jwt",
                "unauthorized",
                "token invalido",
                "token inválido",
            ][..],
        ),
        (
            "chave_publica",
            &["publickey", "public key", "chavepublica", "pem", "asn1"][..],
        ),
        (
            "sistema_operacional",
            &["sistemaoperacional", "operating system"][..],
        ),
        (
            "campo_obrigatorio",
            &["required", "obrigatorio", "obrigatório"][..],
        ),
        (
            "duplicidade",
            &["duplicate", "duplicado", "already exists"][..],
        ),
    ]
    .into_iter()
    .filter(|(_, words)| words.iter().any(|word| lower.contains(word)))
    .map(|(label, _)| label)
    .collect();
    let exceptions: Vec<_> = [
        "NullReferenceException",
        "ArgumentException",
        "FormatException",
        "JsonSerializationException",
        "SecurityTokenException",
        "InvalidOperationException",
    ]
    .into_iter()
    .filter(|name| lower.contains(&name.to_ascii_lowercase()))
    .collect();
    format!(
        "corpo JSON; campos conhecidos={present:?}; indícios={categories:?}; exceções conhecidas={exceptions:?}; valores omitidos"
    )
}

impl Api {
    pub fn validate_base(server: &str) -> Result<Url> {
        let url = Url::parse(server).context("URL do servidor inválida")?;
        ensure!(
            url.scheme() == "https" && url.host_str().is_some(),
            "Servidor precisa usar HTTPS"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "Informe somente a origem HTTPS, sem credenciais, caminho ou parâmetros"
        );
        Ok(url)
    }
    pub fn new(server: &str) -> Result<Self> {
        Self::with_base(Self::validate_base(server)?)
    }
    fn with_base(base: Url) -> Result<Self> {
        let http = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .user_agent("desktopid-linux-research/0.1.0")
            .no_proxy()
            .build()?;
        Ok(Self { base, http })
    }
    fn endpoint(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("URL inválida"))?;
            path.clear();
            for segment in segments {
                ensure!(
                    !segment.is_empty()
                        && *segment != "."
                        && *segment != ".."
                        && !segment.contains('/')
                        && !segment.contains('\\'),
                    "Segmento de rota inválido"
                );
                path.push(segment);
            }
        }
        Ok(url)
    }
    fn post(&self, segments: &[&str], body: &Value, auth: Option<&str>) -> Result<Option<Value>> {
        let mut request = self.http.post(self.endpoint(segments)?).json(body);
        if let Some(auth) = auth {
            let mut header = HeaderValue::from_str(auth).context("Autorização inválida")?;
            header.set_sensitive(true);
            request = request.header(AUTHORIZATION, header);
        }
        let response = request.send().map_err(|_| anyhow::anyhow!("Falha de rede/TLS; resultado da operação desconhecido. Não houve repetição automática"))?;
        let status = response.status();
        if status.as_u16() == 304 {
            return Ok(None);
        }
        if !status.is_success() {
            let mut bytes = Zeroizing::new(Vec::new());
            let summary = if response.take(65537).read_to_end(&mut bytes).is_err() {
                "corpo de erro incompleto; valores omitidos".into()
            } else if bytes.len() > 65536 {
                "corpo de erro excede 64 KiB; valores omitidos".into()
            } else {
                error_summary(&bytes)
            };
            return Err(ServiceError {
                status: status.as_u16(),
                body: bytes,
                summary,
            }
            .into());
        }
        if let Some(size) = response.content_length() {
            ensure!(size <= MAX_RESPONSE, "Resposta excede o limite");
        }
        let mut bytes = Zeroizing::new(Vec::new());
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .context("Resposta incompleta; não repetir automaticamente")?;
        ensure!(
            bytes.len() as u64 <= MAX_RESPONSE,
            "Resposta excede o limite"
        );
        Ok(Some(
            serde_json::from_slice(&bytes).context("Resposta JSON inválida")?,
        ))
    }
    fn required_post(&self, segments: &[&str], body: &Value, auth: Option<&str>) -> Result<Value> {
        self.post(segments, body, auth)?
            .context("304 inesperado nesta operação")
    }
    fn signed_post(&self, segments: &[&str], body: &Value, identity: &Identity) -> Result<Value> {
        let auth = Zeroizing::new(identity.authorization(body)?);
        self.required_post(segments, body, Some(&auth))
    }
    pub fn login(&self, email: &str, access_url: &str, password: &str) -> Result<Login> {
        let mut body = json!({"email": email, "urlAcesso": access_url, "senha": password});
        let result = self.required_post(
            &["api", "manager", "usuarios", "login", "usrsenha"],
            &body,
            None,
        );
        if let Some(password) = body.get_mut("senha").and_then(|v| {
            if let Value::String(s) = v {
                Some(s)
            } else {
                None
            }
        }) {
            zeroize::Zeroize::zeroize(password);
        }
        let mut response = result?;
        let token = Zeroizing::new(text_field(&response, "token")?);
        if let Some(Value::String(value)) = response.get_mut("token") {
            zeroize::Zeroize::zeroize(value);
        }
        Ok(Login {
            user_id: text_field(&response, "id")?,
            organization_id: text_field(&response, "organizacaoId")?,
            token,
        })
    }
    pub fn register(
        &self,
        login: &Login,
        info: &DesktopInfo<'_>,
        identity: &Identity,
    ) -> Result<String> {
        let domain = if info.domain.is_empty() {
            DEFAULT_NETWORK_DOMAIN
        } else {
            info.domain
        };
        let body = json!({"nomeDesktop": info.name, "sistemaOperacional": "Linux", "nomeUsuarioLocal": info.local_user,
            "dominioRede": domain, "chavePublica": identity.public_pem()?});
        // No space after "Bearer": the registration call concatenates the bare
        // literal L"Bearer" with the login token, unlike every signed request,
        // which uses L"Bearer ". Corroborated in both products:
        // macOS 2.2.0.1 callRemoteIdToCreateInstallation loads L"Bearer"
        // @0x296f18 at 0x4f320, while the signed transmitter @0x12616 loads
        // L"Bearer " @0x294b9c; the Windows 2.1.4.3 GUI holds both literals and
        // references the bare one only at 0x14004c183, inside the same function
        // that builds the registration route at 0x14004ba75.
        let auth = Zeroizing::new(format!("Bearer{}", *login.token));
        let result = self.required_post(
            &[
                "api",
                "manager",
                "desktopid",
                "usuario",
                &login.user_id,
                "organizacao",
                &login.organization_id,
            ],
            &body,
            Some(&auth),
        )?;
        text_field(&result, "codigoDesktop")
    }
    /// Read only the logged-in user's profile. Route recovered from the public
    /// manager frontend's visualizarProprioUsuarioOrgPorId(user, organization).
    /// Compare header spacing without invoking any registration endpoint.
    pub fn authorization_diagnostic(&self, login: &Login, spaced: bool) -> Result<Value> {
        let auth = Zeroizing::new(format!(
            "Bearer{}{}",
            if spaced { " " } else { "" },
            *login.token
        ));
        let mut header = HeaderValue::from_str(&auth).context("Autorização inválida")?;
        header.set_sensitive(true);
        let response = self
            .http
            .get(self.endpoint(&[
                "api",
                "manager",
                "usuarios",
                "visualizarproprio",
                &login.user_id,
                &login.organization_id,
            ])?)
            .header(AUTHORIZATION, header)
            .send()
            .map_err(|_| anyhow::anyhow!("Falha de rede/TLS na consulta do próprio perfil"))?;
        let status = response.status();
        let mut bytes = Zeroizing::new(Vec::new());
        response
            .take(65537)
            .read_to_end(&mut bytes)
            .context("Consulta de perfil incompleta")?;
        let mut result = json!({"header_style":if spaced {"bearer-with-space"} else {"legacy-no-space"},
            "http_status":status.as_u16(),"registration_requested":false});
        if bytes.len() > 65536 {
            result["summary"] = json!("resposta excede 64 KiB; valores omitidos");
        } else if !status.is_success() {
            result["summary"] = json!(error_summary(&bytes));
        } else {
            let value = serde_json::from_slice::<Value>(&bytes).ok();
            result["json_response"] = json!(value.is_some());
            result["profile_user_id_matches_login"] = match value
                .as_ref()
                .and_then(|value| text_field(value, "id").ok())
            {
                Some(id) => json!(id == login.user_id),
                None => Value::Null,
            };
        }
        Ok(result)
    }
    pub fn wallet(
        &self,
        desktop: &str,
        identity: &Identity,
    ) -> Result<Option<Vec<WalletCertificate>>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .to_string();
        let body = json!({"momento": timestamp});
        let auth = Zeroizing::new(identity.authorization(&body)?);
        let Some(response) = self.post(
            &["api", "manager", "desktopid", desktop, "carteira"],
            &body,
            Some(&auth),
        )?
        else {
            return Ok(None);
        };
        let certificates = response
            .get("certificados")
            .and_then(Value::as_array)
            .context("Carteira sem array certificados")?;
        certificates
            .iter()
            .map(|item| {
                let encoded = text_field(item, "base64")?;
                let clean: String = encoded
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .collect();
                let der = STANDARD
                    .decode(clean)
                    .context("Certificado Base64 inválido")?;
                Ok(WalletCertificate {
                    key_name: text_field(item, "keyName")?,
                    certificate: X509::from_der(&der)?,
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }
    pub fn open_session(
        &self,
        desktop: &str,
        reference: &CertificateReference<'_>,
        pin: &str,
        otp: &str,
        push: bool,
        identity: &Identity,
    ) -> Result<Zeroizing<String>> {
        let mut body =
            protocol::session_payload(desktop, reference, pin, otp, push, "Axia RemotedID");
        let result = self.signed_post(&["api", "signature", "tokensessao"], &body, identity);
        for field in ["pin", "otp"] {
            if let Some(Value::String(value)) = body.get_mut(field) {
                zeroize::Zeroize::zeroize(value);
            }
        }
        let mut response = result?;
        ensure!(
            response.get("status") == Some(&Value::Bool(true)),
            "Serviço recusou a sessão; confira PIN/OTP ou push"
        );
        let token = Zeroizing::new(text_field(&response, "token")?);
        if let Some(Value::String(value)) = response.get_mut("token") {
            zeroize::Zeroize::zeroize(value);
        }
        Ok(token)
    }
    pub fn sign_digest(
        &self,
        desktop: &str,
        reference: &CertificateReference<'_>,
        token: &str,
        algorithm: Algorithm,
        digest: &[u8],
        identity: &Identity,
    ) -> Result<Vec<u8>> {
        protocol::decode_signature(
            &self
                .request_digest_signature(desktop, reference, token, algorithm, digest, identity)?,
        )
    }

    pub(crate) fn request_digest_signature(
        &self,
        desktop: &str,
        reference: &CertificateReference<'_>,
        token: &str,
        algorithm: Algorithm,
        digest: &[u8],
        identity: &Identity,
    ) -> Result<Value> {
        let mut body = protocol::signature_payload(desktop, reference, token, algorithm, digest)?;
        let result = self.signed_post(
            &["api", "signature", "requestHashSessionSignature"],
            &body,
            identity,
        );
        if let Some(Value::String(value)) = body.get_mut("sessionToken") {
            zeroize::Zeroize::zeroize(value);
        }
        result
    }
}
