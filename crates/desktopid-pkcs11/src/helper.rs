use crate::core::State;
use base64::{Engine, engine::general_purpose::STANDARD};
use cryptoki_sys::*;
use desktopid_client::protocol::Algorithm;
use std::{
    fs,
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(PartialEq, Eq)]
struct Executable {
    path: PathBuf,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
fn executable() -> Result<Executable, CK_RV> {
    let path = std::env::var_os("DESKTOPID_HELPER")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home).join(".local/libexec/desktopid-linux/desktopid-client")
            })
        })
        .ok_or(CKR_FUNCTION_FAILED)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| CKR_FUNCTION_FAILED)?;
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.mode() & 0o022 != 0
        || (metadata.uid() != unsafe { libc::geteuid() } && metadata.uid() != 0)
    {
        return Err(CKR_FUNCTION_FAILED);
    }
    Ok(Executable {
        path,
        inode: metadata.ino(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

struct Broker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    executable: Executable,
    root: PathBuf,
}
impl Broker {
    fn start(executable: Executable, root: &Path) -> Result<Self, CK_RV> {
        let mut child = Command::new(&executable.path)
            .arg("--state-dir")
            .arg(root)
            .arg("pkcs11-broker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| CKR_FUNCTION_FAILED)?;
        let input = child.stdin.take().ok_or(CKR_FUNCTION_FAILED)?;
        let output = child.stdout.take().ok_or(CKR_FUNCTION_FAILED)?;
        let mut broker = Self {
            child,
            input,
            output,
            executable,
            root: root.to_owned(),
        };
        let fd = broker.output.as_raw_fd();
        // SAFETY: fd is the owned pipe from a live child process.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            let _ = broker.child.kill();
            return Err(CKR_FUNCTION_FAILED);
        }
        Ok(broker)
    }
    fn request(&mut self, request: &serde_json::Value) -> Result<Vec<u8>, CK_RV> {
        let mut bytes = serde_json::to_vec(request).map_err(|_| CKR_FUNCTION_FAILED)?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .map_err(|_| CKR_FUNCTION_FAILED)?;
        let deadline = Instant::now() + Duration::from_secs(400);
        let mut output = Vec::new();
        loop {
            if Instant::now() >= deadline {
                return Err(CKR_FUNCTION_CANCELED);
            }
            let mut descriptor = libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: descriptor is one valid pipe; bound waits to check deadline.
            let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(CKR_FUNCTION_FAILED);
            }
            if ready == 0 {
                continue;
            }
            let mut buffer = [0u8; 1024];
            match self.output.read(&mut buffer) {
                Ok(0) => {
                    // EOF can precede the exit status by a few ms. Observe it
                    // without ever resending the signing request.
                    let deadline = Instant::now() + Duration::from_millis(100);
                    let status = loop {
                        let status = self.child.try_wait().map_err(|_| CKR_FUNCTION_FAILED)?;
                        if status.is_some() || Instant::now() >= deadline {
                            break status;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    };
                    return Err(if status.is_some_and(|status| status.code() == Some(2)) {
                        CKR_FUNCTION_CANCELED
                    } else {
                        CKR_FUNCTION_FAILED
                    });
                }
                Ok(count) => {
                    output.extend_from_slice(&buffer[..count]);
                    if output.len() > 8192 {
                        return Err(CKR_FUNCTION_FAILED);
                    }
                    if let Some(end) = output.iter().position(|byte| *byte == b'\n') {
                        if end + 1 != output.len() {
                            return Err(CKR_FUNCTION_FAILED);
                        }
                        let reply: serde_json::Value = serde_json::from_slice(&output[..end])
                            .map_err(|_| CKR_FUNCTION_FAILED)?;
                        if reply["schema_version"] != 1 {
                            return Err(CKR_FUNCTION_FAILED);
                        }
                        if reply["error"] == "cancelled" {
                            return Err(CKR_FUNCTION_CANCELED);
                        }
                        if reply.get("error").is_some() {
                            return Err(CKR_FUNCTION_FAILED);
                        }
                        return STANDARD
                            .decode(
                                reply["signature_base64"]
                                    .as_str()
                                    .ok_or(CKR_FUNCTION_FAILED)?,
                            )
                            .map_err(|_| CKR_FUNCTION_FAILED);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(_) => return Err(CKR_FUNCTION_FAILED),
            }
        }
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
static BROKER: Mutex<Option<Broker>> = Mutex::new(None);

pub fn clear_authorization() {
    if let Ok(mut broker) = BROKER.lock() {
        *broker = None;
    }
}

pub fn helper_signature(
    root: &Path,
    certificate_fingerprint: &str,
    algorithm: Algorithm,
    digest: &[u8],
) -> Result<Vec<u8>, CK_RV> {
    let executable = executable()?;
    let mut broker = BROKER.lock().map_err(|_| CKR_FUNCTION_FAILED)?;
    let reusable = match broker.as_mut() {
        Some(child) => {
            child.executable == executable
                && child.root == root
                && child
                    .child
                    .try_wait()
                    .map_err(|_| CKR_FUNCTION_FAILED)?
                    .is_none()
        }
        None => false,
    };
    if !reusable {
        *broker = None;
        *broker = Some(Broker::start(executable, root)?);
    }
    // No secrets in argv, environment or IPC. Session tokens stay in the child.
    let request = serde_json::json!({"schema_version":1,"certificate_sha256":certificate_fingerprint,
        "algorithm":algorithm.name(),"digest_base64":STANDARD.encode(digest)});
    let result = broker
        .as_mut()
        .ok_or(CKR_FUNCTION_FAILED)?
        .request(&request);
    if result.is_err() {
        *broker = None;
    }
    result
}

pub fn fingerprint(state: &State, index: usize) -> Result<String, CK_RV> {
    let digest = state.certificates[index]
        .certificate
        .digest(openssl::hash::MessageDigest::sha256())
        .map_err(|_| CKR_DEVICE_ERROR)?;
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
