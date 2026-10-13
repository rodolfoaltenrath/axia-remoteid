use crate::core::State;
use base64::{Engine, engine::general_purpose::STANDARD};
use cryptoki_sys::*;
use desktopid_client::protocol::Algorithm;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn helper_signature(
    root: &std::path::Path,
    certificate_fingerprint: &str,
    algorithm: Algorithm,
    digest: &[u8],
) -> Result<Vec<u8>, CK_RV> {
    let path = std::env::var_os("DESKTOPID_HELPER")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home).join(".local/libexec/desktopid-linux/desktopid-client")
            })
        })
        .ok_or(CKR_FUNCTION_FAILED)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| CKR_FUNCTION_FAILED)?;
    // Trusted executable, never a shell command; no secrets in argv/env/stdin.
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.mode() & 0o022 != 0
        || (metadata.uid() != unsafe { libc::geteuid() } && metadata.uid() != 0)
    {
        return Err(CKR_FUNCTION_FAILED);
    }
    let request = serde_json::json!({"schema_version":1,"certificate_sha256":certificate_fingerprint,
        "algorithm":algorithm.name(),"digest_base64":STANDARD.encode(digest)});
    let mut child = Command::new(path)
        .arg("--state-dir")
        .arg(root)
        .arg("pkcs11-sign")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CKR_FUNCTION_FAILED)?;
    let sent = child
        .stdin
        .take()
        .ok_or(CKR_FUNCTION_FAILED)
        .and_then(|mut input| {
            input
                .write_all(&serde_json::to_vec(&request).map_err(|_| CKR_FUNCTION_FAILED)?)
                .map_err(|_| CKR_FUNCTION_FAILED)
        });
    if sent.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CKR_FUNCTION_FAILED);
    }
    let deadline = Instant::now() + Duration::from_secs(400);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CKR_FUNCTION_CANCELED);
            }
        }
    };
    if !status.success() {
        return Err(if status.code() == Some(2) {
            CKR_FUNCTION_CANCELED
        } else {
            CKR_FUNCTION_FAILED
        });
    }
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or(CKR_FUNCTION_FAILED)?
        .take(8193)
        .read_to_end(&mut output)
        .map_err(|_| CKR_FUNCTION_FAILED)?;
    if output.len() > 8192 {
        return Err(CKR_FUNCTION_FAILED);
    }
    let reply: serde_json::Value =
        serde_json::from_slice(&output).map_err(|_| CKR_FUNCTION_FAILED)?;
    if reply["schema_version"] != 1 {
        return Err(CKR_FUNCTION_FAILED);
    }
    STANDARD
        .decode(
            reply["signature_base64"]
                .as_str()
                .ok_or(CKR_FUNCTION_FAILED)?,
        )
        .map_err(|_| CKR_FUNCTION_FAILED)
}

pub fn fingerprint(state: &State, index: usize) -> Result<String, CK_RV> {
    let digest = state.certificates[index]
        .certificate
        .digest(openssl::hash::MessageDigest::sha256())
        .map_err(|_| CKR_DEVICE_ERROR)?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}
