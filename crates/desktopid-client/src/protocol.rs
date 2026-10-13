use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::{
    hash::MessageDigest,
    md::{Md, MdRef},
    pkey::{PKeyRef, Public},
    pkey_ctx::PkeyCtx,
    rsa::Padding,
};
use serde_json::{Value, json};

/// Observed flattenJsonValues @0x11fab; this is not JSON canonicalization.
pub fn flatten_values(body: &Value) -> Result<Vec<u8>> {
    let object = body.as_object().context("Esperado objeto JSON")?;
    let mut keys: Vec<_> = object.keys().collect();
    keys.sort();
    let mut bytes = Vec::new();
    for key in keys {
        match &object[key] {
            Value::String(value) => bytes.extend_from_slice(value.as_bytes()),
            Value::Bool(true) => bytes.extend_from_slice(key.as_bytes()),
            Value::Bool(false) | Value::Null => {}
            Value::Number(value) => bytes.extend_from_slice(
                value
                    .as_i64()
                    .context("Número fora do domínio int64 recuperado")?
                    .to_string()
                    .as_bytes(),
            ),
            Value::Object(_) => bytes.extend(flatten_values(&object[key])?),
            Value::Array(items) => {
                for item in items {
                    bytes.extend(flatten_values(item)?);
                }
            }
        }
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
// SHA1 was observed in the original provider, but Fedora's default crypto
// policy rejects RSA/SHA1 signatures. Do not expose it as an operational choice.
pub enum Algorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl Algorithm {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "SHA256",
            Self::Sha384 => "SHA384",
            Self::Sha512 => "SHA512",
        }
    }
    pub fn digest_len(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
    pub fn message_digest(self) -> MessageDigest {
        match self {
            Self::Sha256 => MessageDigest::sha256(),
            Self::Sha384 => MessageDigest::sha384(),
            Self::Sha512 => MessageDigest::sha512(),
        }
    }
    fn md(self) -> &'static MdRef {
        match self {
            Self::Sha256 => Md::sha256(),
            Self::Sha384 => Md::sha384(),
            Self::Sha512 => Md::sha512(),
        }
    }
}

pub struct CertificateReference<'a> {
    // Explicit values until the original issuer/serial encoding is confirmed.
    pub issue: &'a str,
    pub serial: &'a str,
}

pub fn session_payload(
    desktop: &str,
    reference: &CertificateReference<'_>,
    pin: &str,
    otp: &str,
    push: bool,
    application: &str,
) -> Value {
    json!({"desktopCode": desktop, "pin": pin, "otp": otp, "push": push,
        "nomeAplicacaoDesktop": application, "issue": reference.issue, "serialNumber": reference.serial})
}

pub fn signature_payload(
    desktop: &str,
    reference: &CertificateReference<'_>,
    token: &str,
    algorithm: Algorithm,
    digest: &[u8],
) -> Result<Value> {
    ensure!(
        digest.len() == algorithm.digest_len(),
        "Tamanho do digest incompatível com o algoritmo"
    );
    Ok(
        json!({"desktopCode": desktop, "sessionToken": token, "issue": reference.issue,
        "serialNumber": reference.serial, "algorithm": algorithm.name(),
        "hashArray": [{"id": 0, "hash": STANDARD.encode(digest)}]}),
    )
}

pub fn decode_signature(response: &Value) -> Result<Vec<u8>> {
    ensure!(
        response.get("status") == Some(&Value::Bool(true)),
        "Serviço recusou a assinatura"
    );
    let items = response
        .get("idArray")
        .and_then(Value::as_array)
        .context("idArray ausente")?;
    ensure!(items.len() == 1, "Esperada exatamente uma assinatura");
    let item = &items[0];
    ensure!(
        item.get("status") == Some(&Value::Bool(true)),
        "Assinatura individual recusada"
    );
    // Original signHash @0x62174..0x621f2 reads idArray[0].status and
    // signatureBase64, never the response id. The request id remains integer 0
    // (@0x61f51 calls Json::Value(int)). Do not impose an undocumented echo-ID
    // requirement. Callers MUST verify against the requested digest/certificate.
    let value = item
        .get("signatureBase64")
        .and_then(Value::as_str)
        .context("Assinatura ausente")?;
    let signature = STANDARD
        .decode(value)
        .context("Base64 de assinatura inválido")?;
    ensure!(!signature.is_empty(), "Assinatura vazia");
    Ok(signature)
}

/// Fixed structural metadata only: no IDs, messages, tokens or signature bytes.
pub fn signature_response_summary(response: &Value) -> Value {
    let items = response.get("idArray").and_then(Value::as_array);
    let item = items
        .filter(|items| items.len() == 1)
        .and_then(|items| items.first());
    let id = item.and_then(|item| item.get("id"));
    let id_type = match id {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::String(_)) => "string",
        Some(Value::Number(number)) if number.is_i64() || number.is_u64() => "integer",
        Some(Value::Number(_)) => "number",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    };
    json!({"service_status":response.get("status").and_then(Value::as_bool),
        "result_count":items.map(Vec::len),
        "individual_status":item.and_then(|item| item.get("status")).and_then(Value::as_bool),
        "id_type":id_type, "id_is_numeric_zero":id.and_then(Value::as_i64) == Some(0),
        "signature_base64_is_string":item.and_then(|item| item.get("signatureBase64")).is_some_and(Value::is_string)})
}

/// Verify a PREHASHED digest, not SHA256(digest); certificate trust is separate.
pub fn verify_digest(
    key: &PKeyRef<Public>,
    algorithm: Algorithm,
    digest: &[u8],
    signature: &[u8],
) -> Result<()> {
    ensure!(
        digest.len() == algorithm.digest_len(),
        "Tamanho de digest inválido"
    );
    ensure!(
        key.rsa().is_ok(),
        "Somente certificados RSA são suportados nesta versão"
    );
    let mut ctx = PkeyCtx::new(key)?;
    ctx.verify_init()?;
    ctx.set_rsa_padding(Padding::PKCS1)?;
    ctx.set_signature_md(algorithm.md())?;
    if !ctx.verify(digest, signature).unwrap_or(false) {
        bail!("Assinatura não corresponde ao digest e certificado selecionados");
    }
    Ok(())
}

/// Session tokens are cached by the original provider for 30 minutes.
///
/// `TokenStore::addToken` @0x4b042 keys the cache by
/// `(system_clock::now() + 1_800_000_000us) / 100_000us`, i.e. the expiry
/// instant in 100ms units, and `whenTokenFound` @0x4b180 erases every entry
/// below `now / 100_000us` before looking the key up. That is the client-side
/// lifetime only: the service may invalidate a token earlier.
pub const SESSION_CACHE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Split a wallet `keyName` into the literal `serialNumber` and `issue` values.
///
/// `RemoteIdSignatureStrategy::openSession` @0x60ed2 and `::signHash` @0x61c50
/// both run `boost::algorithm::split(parts, keyName, is_any_of(";"),
/// token_compress_on)` and then assign `parts[0]` to `serialNumber` and
/// `parts[1]` to `issue`. The order is deliberate and is NOT the order used by
/// the X.509 display fields, so never derive these from the certificate.
///
/// Both call sites split a `std::wstring` into `std::vector<std::string>`,
/// which narrows each character to `char`. Non-ASCII input would therefore be
/// mangled by the original, so it is rejected here instead of diverging.
pub fn reference_from_key_name(key_name: &str) -> Result<(String, String)> {
    ensure!(
        key_name.is_ascii(),
        "keyName com caracteres não ASCII; o provedor original estreita wchar_t para char e não há representação confirmada"
    );
    // The original compresses runs of ';'; empty segments are dropped here and
    // an exact pair is required, which rejects the ambiguous edge cases.
    let parts: Vec<&str> = key_name.split(';').filter(|p| !p.is_empty()).collect();
    ensure!(
        parts.len() == 2,
        "keyName deve conter exatamente série e emissor separados por ';' (recebido: {} parte(s))",
        parts.len()
    );
    Ok((parts[0].to_owned(), parts[1].to_owned()))
}
