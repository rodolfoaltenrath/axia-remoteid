//! Test processes and synthetic state. Never sets environment on the parent.
use crate::publication::digest;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::x509::X509;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn fixture(root: &Path) -> Result<Value> {
    let value: Value =
        serde_json::from_slice(&fs::read(root.join(crate::publication::LAB_FIXTURE))?)?;
    ensure!(
        value["laboratory_only"] == true,
        "Fixture de laboratório obrigatório"
    );
    Ok(value)
}

pub fn decode(value: &Value) -> Result<Vec<u8>> {
    Ok(STANDARD.decode(
        value
            .as_str()
            .context("Valor Base64 de laboratório ausente")?,
    )?)
}

pub fn private_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(data)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub fn json_write(path: &Path, value: &Value) -> Result<()> {
    private_write(path, &serde_json::to_vec_pretty(value)?)
}

pub fn report(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    json_write(path, value)
}

pub fn mode(path: &Path) -> Result<u32> {
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o777)
}

pub fn require_container() -> Result<()> {
    ensure!(
        Path::new("/run/.containerenv").exists() || Path::new("/.dockerenv").exists(),
        "Execute somente em contêiner descartável"
    );
    Ok(())
}

pub fn command(binary: &Path, home: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("DESKTOPID_STATE_DIR", home.join("identity"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DESKTOPID_HELPER")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DBUS_STARTER_ADDRESS")
        .env_remove("XDG_RUNTIME_DIR");
    command
}

pub struct Process {
    child: Child,
    stopped: bool,
}

impl Process {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        // SAFETY: setpgid is async-signal-safe; only the new child is affected.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        Ok(Self {
            child: command
                .spawn()
                .context("Não foi possível iniciar processo de teste")?,
            stopped: false,
        })
    }
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        // SAFETY: negative pid addresses only the process group made above.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn capture(command: &mut Command, timeout: Duration, input: &[u8]) -> Result<Output> {
    command.stdin(Stdio::piped());
    capture_terminal(command, timeout, input)
}

/// Preserve an explicitly supplied PTY for tests of interactive-only commands.
pub fn capture_terminal(command: &mut Command, timeout: Duration, input: &[u8]) -> Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = Process::spawn(command)?;
    let stdout = process
        .child()
        .stdout
        .take()
        .context("stdout do teste ausente")?;
    let stderr = process
        .child()
        .stderr
        .take()
        .context("stderr do teste ausente")?;
    // Drain both pipes concurrently so a verbose child cannot deadlock. Limit
    // retained diagnostics to 4 MiB while continuing to drain the stream.
    fn drain(mut stream: impl Read) -> std::io::Result<Vec<u8>> {
        let mut data = Vec::new();
        let mut block = [0; 8192];
        loop {
            let n = stream.read(&mut block)?;
            if n == 0 {
                break;
            }
            let keep = n.min((4 * 1024 * 1024usize).saturating_sub(data.len()));
            data.extend_from_slice(&block[..keep]);
        }
        Ok(data)
    }
    let out = thread::spawn(move || drain(stdout));
    let err = thread::spawn(move || drain(stderr));
    if let Some(mut stdin) = process.child().stdin.take() {
        stdin.write_all(input)?;
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = process.child().try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    process.stop();
    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("Leitor stdout falhou"))??;
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("Leitor stderr falhou"))??;
    Ok(Output {
        status: status.context("Tempo do teste excedido; nenhuma repetição automática")?,
        stdout,
        stderr,
    })
}

pub fn successful(command: &mut Command) -> Result<Output> {
    let result = capture(command, Duration::from_secs(30), b"")?;
    ensure!(
        result.status.success(),
        "Processo de laboratório falhou: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result)
}

pub fn initialize(binary: &Path, home: &Path, state: &Path, fixture: &Value) -> Result<()> {
    successful(command(binary, home).arg("--state-dir").arg(state).args([
        "init",
        "--server",
        "https://example.invalid",
        "--access-url",
        "",
    ]))?;
    let mut config: Value = serde_json::from_slice(&fs::read(state.join("config.json"))?)?;
    config["desktop_code"] = json!("synthetic-lab-code");
    json_write(&state.join("config.json"), &config)?;
    let cert = X509::from_pem(
        fixture["certificate_pem"]
            .as_str()
            .context("Certificado de laboratório ausente")?
            .as_bytes(),
    )?;
    json_write(
        &state.join("wallet.json"),
        &json!({"schema_version": 1, "public_key_sha256": config["public_key_sha256"], "desktop_code": config["desktop_code"], "certificados": [{"index":0, "keyName":"SERIAL;ISSUER", "base64": STANDARD.encode(cert.to_der()?)}]}),
    )
}

pub fn snapshot(directory: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fs::read_dir(directory)?
        .map(|entry| {
            let path = entry?.path();
            ensure!(path.is_file(), "Snapshot exige arquivos regulares");
            Ok((path.clone(), digest(&fs::read(path)?)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_propagates_failure_and_enforces_timeout() -> Result<()> {
        let result = capture(&mut Command::new("/bin/false"), Duration::from_secs(2), b"")?;
        assert!(!result.status.success());
        let before = Instant::now();
        assert!(
            capture(
                Command::new("/bin/sleep").arg("10"),
                Duration::from_millis(50),
                b""
            )
            .is_err()
        );
        assert!(before.elapsed() < Duration::from_secs(2));
        Ok(())
    }
    #[test]
    fn failed_cargo_suite_cannot_be_hidden_by_successful_looking_output() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let temp = tempfile::tempdir()?;
        let cargo = temp.path().join("cargo");
        private_write(&cargo, b"#!/bin/sh\ncase \"$1\" in\nfmt|clippy) exit 0;;\ntest) printf 'test result: ok. 999 passed\\n'; exit 7;;\n*) touch \"$CARGO_TARGET_DIR/unexpected-build\"; exit 0;;\nesac\n")?;
        fs::set_permissions(cargo, fs::Permissions::from_mode(0o700))?;
        let paths = std::env::join_paths(std::iter::once(temp.path().to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))?;
        let output = capture(
            Command::new("sh")
                .arg(root.join("scripts/test_rust_wsl.sh"))
                .env("PATH", paths)
                .env("CARGO_TARGET_DIR", temp.path()),
            Duration::from_secs(15),
            b"",
        )?;
        ensure!(
            !output.status.success()
                && !temp.path().join("unexpected-build").exists()
                && !String::from_utf8_lossy(&output.stdout).contains("Linux executable:"),
            "Controlador ocultou falha Cargo"
        );
        Ok(())
    }
}
