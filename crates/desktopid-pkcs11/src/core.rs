use cryptoki_sys::*;
use desktopid_client::{
    client::WalletCertificate,
    protocol::{Algorithm, reference_from_key_name, verify_digest},
    storage::Store,
};
use openssl::{hash::hash, nid::Nid};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub const SLOT: CK_SLOT_ID = 8;
pub const MAX_INPUT: usize = 16 * 1024 * 1024;
pub const MECHANISMS: &[CK_MECHANISM_TYPE] = &[
    CKM_RSA_PKCS,
    CKM_SHA256_RSA_PKCS,
    CKM_SHA384_RSA_PKCS,
    CKM_SHA512_RSA_PKCS,
];
pub static STATE: Mutex<Option<State>> = Mutex::new(None);
pub static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub struct Object {
    pub attrs: BTreeMap<CK_ATTRIBUTE_TYPE, Vec<u8>>,
    pub certificate: usize,
    pub private_key: bool,
}
pub struct Operation {
    pub mechanism: CK_MECHANISM_TYPE,
    pub certificate: usize,
    pub data: Vec<u8>,
    pub multipart: bool,
}
pub struct Search {
    pub matches: Vec<CK_OBJECT_HANDLE>,
    pub offset: usize,
}
#[derive(Default)]
pub struct Session {
    pub flags: CK_FLAGS,
    pub search: Option<Search>,
    pub sign: Option<Operation>,
    pub verify: Option<Operation>,
    pub busy: bool,
}
pub struct State {
    pub root: PathBuf,
    pub certificates: Vec<WalletCertificate>,
    pub objects: BTreeMap<CK_OBJECT_HANDLE, Object>,
    pub sessions: BTreeMap<CK_SESSION_HANDLE, Session>,
    pub logged_in: bool,
}
pub fn root_path() -> Option<PathBuf> {
    std::env::var_os("DESKTOPID_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
                .map(|p| p.join("desktopid-linux"))
        })
}
pub fn ulong(value: CK_ULONG) -> Vec<u8> {
    value.to_ne_bytes().to_vec()
}
pub fn boolean(value: bool) -> Vec<u8> {
    vec![u8::from(value)]
}
fn der_integer(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_empty() {
        bytes.push(0);
    }
    if bytes[0] & 0x80 != 0 {
        bytes.insert(0, 0);
    }
    let mut out = vec![2];
    if bytes.len() < 128 {
        out.push(bytes.len() as u8);
    } else {
        let len = bytes.len().to_be_bytes();
        let first = len.iter().position(|b| *b != 0).unwrap_or(len.len() - 1);
        out.push(0x80 | (len.len() - first) as u8);
        out.extend_from_slice(&len[first..]);
    }
    out.extend(bytes);
    out
}
impl State {
    pub fn load() -> Result<Self, CK_RV> {
        let root = root_path().ok_or(CKR_TOKEN_NOT_PRESENT)?;
        if !root.exists() {
            return Err(CKR_TOKEN_NOT_PRESENT);
        }
        let store = Store::new(root.clone()).map_err(|_| CKR_DEVICE_ERROR)?;
        let certificates = store.load_wallet_public().map_err(|_| CKR_DEVICE_ERROR)?;
        let mut objects = BTreeMap::new();
        for (index, entry) in certificates.iter().enumerate() {
            let cert = &entry.certificate;
            let id = cert
                .digest(openssl::hash::MessageDigest::sha256())
                .map_err(|_| CKR_DEVICE_ERROR)?
                .to_vec();
            let label = cert
                .subject_name()
                .entries_by_nid(Nid::COMMONNAME)
                .next()
                .and_then(|e| e.data().to_string().ok())
                .unwrap_or_else(|| format!("Axia RemotedID certificado {}", index + 1));
            let base = BTreeMap::from([
                (CKA_TOKEN, boolean(true)),
                (CKA_MODIFIABLE, boolean(false)),
                (CKA_COPYABLE, boolean(false)),
                (CKA_DESTROYABLE, boolean(false)),
                (CKA_LABEL, label.into_bytes()),
                (CKA_ID, id),
                (CKA_PRIVATE, boolean(false)),
            ]);
            let mut attrs = base.clone();
            attrs.extend([
                (CKA_CLASS, ulong(CKO_CERTIFICATE)),
                (CKA_CERTIFICATE_TYPE, ulong(CKC_X_509)),
                (CKA_VALUE, cert.to_der().map_err(|_| CKR_DEVICE_ERROR)?),
                (
                    CKA_SUBJECT,
                    cert.subject_name().to_der().map_err(|_| CKR_DEVICE_ERROR)?,
                ),
                (
                    CKA_ISSUER,
                    cert.issuer_name().to_der().map_err(|_| CKR_DEVICE_ERROR)?,
                ),
                (
                    CKA_SERIAL_NUMBER,
                    der_integer(
                        cert.serial_number()
                            .to_bn()
                            .map_err(|_| CKR_DEVICE_ERROR)?
                            .to_vec(),
                    ),
                ),
                (CKA_TRUSTED, boolean(false)),
            ]);
            let handle = (index as CK_ULONG) * 3 + 1;
            objects.insert(
                handle,
                Object {
                    attrs,
                    certificate: index,
                    private_key: false,
                },
            );
            let public = cert.public_key().map_err(|_| CKR_DEVICE_ERROR)?;
            let Ok(rsa) = public.rsa() else {
                continue;
            };
            if !(2048..=8192).contains(&public.bits()) {
                continue;
            }
            if reference_from_key_name(&entry.key_name).is_err() {
                continue;
            }
            let mut attrs = base;
            attrs.extend([
                (CKA_KEY_TYPE, ulong(CKK_RSA)),
                (CKA_MODULUS, rsa.n().to_vec()),
                (CKA_MODULUS_BITS, ulong(public.bits().into())),
                (CKA_PUBLIC_EXPONENT, rsa.e().to_vec()),
                (
                    CKA_PUBLIC_KEY_INFO,
                    public.public_key_to_der().map_err(|_| CKR_DEVICE_ERROR)?,
                ),
                (
                    CKA_SUBJECT,
                    cert.subject_name().to_der().map_err(|_| CKR_DEVICE_ERROR)?,
                ),
                (CKA_LOCAL, boolean(false)),
                (CKA_ENCRYPT, boolean(false)),
                (CKA_DECRYPT, boolean(false)),
                (CKA_WRAP, boolean(false)),
                (CKA_UNWRAP, boolean(false)),
                (CKA_DERIVE, boolean(false)),
                (CKA_SIGN_RECOVER, boolean(false)),
                (CKA_VERIFY_RECOVER, boolean(false)),
                (
                    CKA_ALLOWED_MECHANISMS,
                    MECHANISMS.iter().flat_map(|m| m.to_ne_bytes()).collect(),
                ),
            ]);
            let mut pub_attrs = attrs.clone();
            pub_attrs.extend([
                (CKA_CLASS, ulong(CKO_PUBLIC_KEY)),
                (CKA_SIGN, boolean(false)),
                (CKA_VERIFY, boolean(true)),
            ]);
            objects.insert(
                handle + 1,
                Object {
                    attrs: pub_attrs,
                    certificate: index,
                    private_key: false,
                },
            );
            attrs.extend([
                (CKA_CLASS, ulong(CKO_PRIVATE_KEY)),
                // This is a public reference to a remote key, not key material.
                // Authorization belongs to each effective C_Sign operation.
                (CKA_PRIVATE, boolean(false)),
                (CKA_SIGN, boolean(true)),
                (CKA_VERIFY, boolean(false)),
                (CKA_SENSITIVE, boolean(true)),
                (CKA_ALWAYS_SENSITIVE, boolean(true)),
                (CKA_EXTRACTABLE, boolean(false)),
                (CKA_NEVER_EXTRACTABLE, boolean(true)),
                (CKA_ALWAYS_AUTHENTICATE, boolean(false)),
            ]);
            objects.insert(
                handle + 2,
                Object {
                    attrs,
                    certificate: index,
                    private_key: true,
                },
            );
        }
        Ok(Self {
            root,
            certificates,
            objects,
            sessions: BTreeMap::new(),
            logged_in: false,
        })
    }
}
pub fn with_state(f: impl FnOnce(&mut State) -> CK_RV) -> CK_RV {
    let Ok(mut lock) = STATE.lock() else {
        return CKR_DEVICE_ERROR;
    };
    match lock.as_mut() {
        Some(state) => f(state),
        None => CKR_CRYPTOKI_NOT_INITIALIZED,
    }
}
pub fn next_session() -> CK_SESSION_HANDLE {
    NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
}
pub fn input_digest(
    mechanism: CK_MECHANISM_TYPE,
    data: &[u8],
) -> Result<(Algorithm, Vec<u8>), CK_RV> {
    let algorithm = match mechanism {
        CKM_SHA256_RSA_PKCS => Algorithm::Sha256,
        CKM_SHA384_RSA_PKCS => Algorithm::Sha384,
        CKM_SHA512_RSA_PKCS => Algorithm::Sha512,
        CKM_RSA_PKCS => {
            // Only supported DigestInfo encodings, never arbitrary raw RSA data.
            for (algorithm, prefix) in [
                (
                    Algorithm::Sha256,
                    &[
                        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03,
                        0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
                    ][..],
                ),
                (
                    Algorithm::Sha384,
                    &[
                        0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03,
                        0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30,
                    ][..],
                ),
                (
                    Algorithm::Sha512,
                    &[
                        0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03,
                        0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40,
                    ][..],
                ),
            ] {
                if data.len() == prefix.len() + algorithm.digest_len() && data.starts_with(prefix) {
                    return Ok((algorithm, data[prefix.len()..].to_vec()));
                }
            }
            return Err(CKR_DATA_INVALID);
        }
        _ => return Err(CKR_MECHANISM_INVALID),
    };
    Ok((
        algorithm,
        hash(algorithm.message_digest(), data)
            .map_err(|_| CKR_FUNCTION_FAILED)?
            .to_vec(),
    ))
}
pub fn check_signature(
    entry: &WalletCertificate,
    algorithm: Algorithm,
    digest: &[u8],
    signature: &[u8],
) -> Result<(), CK_RV> {
    let public = entry
        .certificate
        .public_key()
        .map_err(|_| CKR_DEVICE_ERROR)?;
    verify_digest(&public, algorithm, digest, signature).map_err(|_| CKR_SIGNATURE_INVALID)
}
