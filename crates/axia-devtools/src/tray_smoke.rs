use crate::{lab, lab_helper};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use zbus::{
    blocking::{Connection, Proxy, connection::Builder},
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Structure, Value as Variant},
};
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
type Properties = HashMap<String, OwnedValue>;
type Layout = (i32, Properties, Vec<OwnedValue>);

struct Watcher {
    registered: Arc<Mutex<Vec<String>>>,
}
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] header: Header<'_>) {
        self.registered
            .lock()
            .unwrap()
            .push(if service.starts_with('/') {
                header.sender().unwrap().to_string()
            } else {
                service.into()
            });
    }
    fn register_status_notifier_host(&self, _service: &str) {}
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

fn wait(mut condition: impl FnMut() -> Result<bool>, label: &str) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if condition()? {
            return Ok(());
        }
        ensure!(Instant::now() < until, "Tray timeout: {label}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn names(connection: &Connection) -> Result<Vec<String>> {
    let names: Vec<String> = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?
    .call("ListNames", &())?;
    Ok(names
        .into_iter()
        .filter(|name| name.starts_with("org.kde.StatusNotifierItem-"))
        .collect())
}

fn entries(
    connection: &Connection,
    destination: &str,
    path: &str,
) -> Result<HashMap<String, (i32, Properties)>> {
    let menu = Proxy::new(connection, destination, path, MENU)?;
    let (_, layout): (u32, Layout) =
        menu.call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))?;
    let mut entries = HashMap::new();
    for value in layout.2 {
        let structure = Structure::try_from(value)?;
        let fields = structure.into_fields();
        ensure!(fields.len() == 3, "Entrada de menu inválida");
        let id = i32::try_from(&fields[0])?;
        let properties = Properties::try_from(fields[1].try_clone()?)?;
        if let Some(label) = properties.get("label") {
            entries.insert(<&str>::try_from(label)?.into(), (id, properties));
        }
    }
    Ok(entries)
}
fn click(connection: &Connection, destination: &str, path: &str, label: &str) -> Result<()> {
    let entries = entries(connection, destination, path)?;
    let (id, _) = entries.get(label).context("Item de menu ausente")?;
    Proxy::new(connection, destination, path, MENU)?
        .call::<_, _, ()>("Event", &(*id, "clicked", Variant::I32(0), 0u32))?;
    Ok(())
}

pub fn smoke(binary: &Path) -> Result<()> {
    ensure!(
        desktopid_client::installation::system_helper().is_none(),
        "Teste de bandeja requer ambiente sem auxiliar RPM instalado; use contêiner descartável"
    );
    let binary = binary.canonicalize()?;
    let home = tempfile::Builder::new()
        .prefix("axia-tray-lab-")
        .tempdir()?;
    lab_helper::mark(home.path())?;
    let result = lab::capture(
        lab::command(Path::new("dbus-run-session"), home.path())
            .arg("--")
            .arg(env::current_exe()?)
            .arg("tray-worker")
            .arg("--binary")
            .arg(binary)
            .arg("--home")
            .arg(home.path()),
        Duration::from_secs(90),
        b"",
    )?;
    ensure!(
        result.status.success(),
        "Tray test falhou: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8_lossy(&result.stdout));
    Ok(())
}

pub fn worker(binary: &Path, home: &Path) -> Result<()> {
    lab_helper::validate_home(home)?;
    let registered = Arc::new(Mutex::new(Vec::<String>::new()));
    let connection = Builder::session()?
        .serve_at(
            "/StatusNotifierWatcher",
            Watcher {
                registered: registered.clone(),
            },
        )?
        .build()?;
    let has_owner: bool = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?
    .call("NameHasOwner", &(WATCHER,))?;
    ensure!(!has_owner, "Não substituir watcher da sessão real");
    let state = home.join(".config/desktopid-linux");
    let marker = home.join("gui-opened");
    let helper = home.join(".local/libexec/desktopid-linux/desktopid-client");
    fs::create_dir_all(helper.parent().unwrap())?;
    fs::copy(env::current_exe()?, &helper)?;
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))?;
    let bus = env::var_os("DBUS_SESSION_BUS_ADDRESS").context("Barramento privado ausente")?;
    let command = || {
        let mut command = lab::command(binary, home);
        command
            .arg("--state-dir")
            .arg(&state)
            .env("DBUS_SESSION_BUS_ADDRESS", &bus)
            .env("AXIA_LAB_HOME", home);
        command
    };
    lab::successful(command().args(["autostart", "--enable"]))?;
    let preferences = home.join(".config/axia-remotedid/preferences.json");
    let before = fs::read(&preferences)?;
    let mut resident_command = command();
    resident_command
        .arg("startup")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut resident = lab::Process::spawn(&mut resident_command)?;
    wait(
        || Ok(names(&connection)?.len() == 1),
        "resident without watcher",
    )?;
    ensure!(
        resident.child().try_wait()?.is_none()
            && !marker.exists()
            && !state.exists()
            && fs::read(&preferences)? == before,
        "Startup abriu GUI ou alterou conta/preferências"
    );
    connection.request_name(WATCHER)?;
    wait(|| Ok(registered.lock().unwrap().len() == 1), "late watcher")?;
    let destination = registered.lock().unwrap()[0].clone();
    let properties: Properties = Proxy::new(
        &connection,
        destination.as_str(),
        "/StatusNotifierItem",
        "org.freedesktop.DBus.Properties",
    )?
    .call("GetAll", &(ITEM,))?;
    ensure!(
        <&str>::try_from(&properties["Id"])? == "axia-remotedid"
            && <&str>::try_from(&properties["Title"])? == "Axia RemotedID"
            && i32::try_from(&properties["WindowId"])? == 0,
        "Tray branding/janela incorretos"
    );
    let menu_path = OwnedObjectPath::try_from(properties["Menu"].try_clone()?)?;
    lab::successful(command().arg("startup"))?;
    ensure!(
        names(&connection)?.len() == 1 && !marker.exists() && !state.exists(),
        "Residente duplicou ou abriu GUI"
    );
    connection.release_name(WATCHER)?;
    thread::sleep(Duration::from_millis(300));
    connection.request_name(WATCHER)?;
    wait(
        || Ok(registered.lock().unwrap().len() == 2),
        "watcher restart",
    )?;
    ensure!(
        names(&connection)?.len() == 1,
        "Host reiniciado duplicou residente"
    );
    Proxy::new(
        &connection,
        destination.as_str(),
        "/StatusNotifierItem",
        ITEM,
    )?
    .call::<_, _, ()>("Activate", &(0i32, 0i32))?;
    wait(|| Ok(marker.exists()), "GUI activation")?;
    click(
        &connection,
        &destination,
        menu_path.as_str(),
        "Abrir Axia RemotedID",
    )?;
    thread::sleep(Duration::from_millis(200));
    ensure!(
        fs::read_to_string(&marker)?.lines().count() == 1,
        "Clique repetido abriu segunda GUI"
    );
    thread::sleep(Duration::from_millis(1600));
    click(
        &connection,
        &destination,
        menu_path.as_str(),
        "Abrir Axia RemotedID",
    )?;
    wait(
        || Ok(fs::read_to_string(&marker)?.lines().count() == 2),
        "reopen GUI",
    )?;
    thread::sleep(Duration::from_millis(1600));
    ensure!(
        resident.child().try_wait()?.is_none(),
        "Fechar GUI encerrou bandeja"
    );
    let enabled = || -> Result<bool> {
        Ok(serde_json::from_slice::<Value>(&fs::read(&preferences)?)?["autostart_enabled"] == true)
    };
    click(
        &connection,
        &destination,
        menu_path.as_str(),
        "Iniciar com o sistema",
    )?;
    wait(|| Ok(!enabled()?), "disable startup")?;
    wait(
        || {
            Ok(i32::try_from(
                &entries(&connection, &destination, menu_path.as_str())?["Iniciar com o sistema"].1
                    ["toggle-state"],
            )? == 0)
        },
        "checkbox refresh",
    )?;
    click(
        &connection,
        &destination,
        menu_path.as_str(),
        "Iniciar com o sistema",
    )?;
    wait(enabled, "enable startup")?;
    click(&connection, &destination, menu_path.as_str(), "Sair")?;
    wait(
        || Ok(resident.child().try_wait()?.is_some()),
        "resident exit",
    )?;
    use std::io::Read;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    resident
        .child()
        .stdout
        .as_mut()
        .unwrap()
        .read_to_end(&mut stdout)?;
    resident
        .child()
        .stderr
        .as_mut()
        .unwrap()
        .read_to_end(&mut stderr)?;
    ensure!(
        stdout.is_empty() && stderr.is_empty() && !state.exists() && enabled()?,
        "Bandeja exibiu saída ou alterou conta"
    );
    wait(|| Ok(names(&connection)?.is_empty()), "tray removed")?;
    let mut restart = command();
    restart
        .arg("startup")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut restart = lab::Process::spawn(&mut restart)?;
    wait(
        || Ok(registered.lock().unwrap().len() == 3),
        "resident restart",
    )?;
    let destination = registered.lock().unwrap().last().unwrap().clone();
    click(&connection, &destination, menu_path.as_str(), "Sair")?;
    wait(
        || Ok(restart.child().try_wait()?.is_some()),
        "restarted exit",
    )?;
    ensure!(!state.exists(), "Reinício criou conta");
    connection.release_name(WATCHER)?;
    println!(
        "Tray: silent startup, late/restarted host, single resident, no window, explicit open/reopen, autostart toggle and exit passed; no real account."
    );
    Ok(())
}

pub fn gui_helper() -> Result<bool> {
    let Some(home) = env::var_os("AXIA_LAB_HOME") else {
        return Ok(false);
    };
    let home = std::path::PathBuf::from(home);
    if env::current_exe()? != home.join(".local/libexec/desktopid-linux/desktopid-client") {
        return Ok(false);
    }
    lab_helper::validate_home(&home)?;
    use std::io::Write;
    let mut output = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("gui-opened"))?;
    writeln!(output, "open")?;
    thread::sleep(Duration::from_millis(1500));
    Ok(true)
}
