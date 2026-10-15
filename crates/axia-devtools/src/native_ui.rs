use crate::lab;
use anyhow::{Result, ensure};
use std::{
    env, fs,
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

pub fn smoke(root: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    let target = target.canonicalize()?;
    let temp = tempfile::Builder::new()
        .prefix("axia-native-ui-lab-")
        .tempdir()?;
    let home = temp.path();
    let runtime = home.join("runtime");
    fs::create_dir(&runtime)?;
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let display = format!(":{}", 100 + std::process::id() % 20000);
    let original_home =
        env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME da ferramenta ausente"))?;
    let mut daemon = lab::command(Path::new("gtk4-broadwayd"), home);
    daemon
        .arg("--address=127.0.0.1")
        .arg(format!("--port={port}"))
        .arg(&display)
        .env("XDG_RUNTIME_DIR", &runtime)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = lab::Process::spawn(&mut daemon)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ensure!(
            process.child().try_wait()?.is_none(),
            "Display privado Broadway não iniciou"
        );
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200))
            .is_ok()
        {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Display privado não ficou pronto"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let mut command = lab::command(Path::new("dbus-run-session"), home);
    command
        .current_dir(root)
        .args([
            "--",
            "cargo",
            "test",
            "-p",
            "axia-desktop",
            "--lib",
            "--locked",
            "ui::tests::native_forms",
            "--",
            "--ignored",
            "--test-threads=1",
        ])
        .env("DISPLAY", &display)
        .env("BROADWAY_DISPLAY", &display)
        .env("GDK_BACKEND", "broadway")
        .env("XDG_RUNTIME_DIR", runtime)
        .env("GTK_A11Y", "none")
        .env("GIO_USE_VFS", "local")
        .env("CARGO_TARGET_DIR", target)
        .env(
            "CARGO_HOME",
            env::var_os("CARGO_HOME")
                .unwrap_or_else(|| Path::new(&original_home).join(".cargo").into_os_string()),
        )
        .env(
            "RUSTUP_HOME",
            env::var_os("RUSTUP_HOME")
                .unwrap_or_else(|| Path::new(&original_home).join(".rustup").into_os_string()),
        );
    let result = lab::capture(&mut command, Duration::from_secs(180), b"")?;
    print!("{}", String::from_utf8_lossy(&result.stdout));
    ensure!(
        result.status.success(),
        "Teste GTK falhou: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    ensure!(
        !home.join(".config/desktopid-linux").exists(),
        "Teste GTK criou identidade"
    );
    println!(
        "Native GTK: private display, cancellation, password buffers, responsive worker and saved-registration recovery passed; no account access."
    );
    Ok(())
}
