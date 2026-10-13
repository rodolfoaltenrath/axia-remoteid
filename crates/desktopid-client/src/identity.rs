use crate::protocol::flatten_values;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::{
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::{Padding, Rsa},
    sign::Signer,
};
use serde_json::Value;
use zeroize::Zeroizing;

pub struct Identity {
    key: PKey<Private>,
}

impl Identity {
    pub fn generate() -> Result<Self> {
        Ok(Self {
            key: PKey::from_rsa(Rsa::generate(2048)?)?,
        })
    }
    pub fn from_pem(pem: &[u8]) -> Result<Self> {
        let key = PKey::private_key_from_pem(pem).context("Chave local inválida")?;
        ensure!(key.rsa()?.size() == 256, "Esperada identidade RSA-2048");
        Ok(Self { key })
    }
    pub fn private_pem(&self) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(self.key.private_key_to_pem_pkcs8()?))
    }
    pub fn public_pem(&self) -> Result<String> {
        Ok(String::from_utf8(self.key.public_key_to_pem()?)?)
    }
    pub fn fingerprint(&self) -> Result<String> {
        let digest = openssl::sha::sha256(&self.key.public_key_to_der()?);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    pub fn authorization(&self, body: &Value) -> Result<String> {
        let bytes = Zeroizing::new(flatten_values(body)?);
        let mut signer = Signer::new(MessageDigest::sha256(), &self.key)?;
        signer.set_rsa_padding(Padding::PKCS1)?;
        signer.update(&bytes)?;
        Ok(format!("Bearer {}", STANDARD.encode(signer.sign_to_vec()?)))
    }
}
