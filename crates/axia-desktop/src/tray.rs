//! Windowless session resident. Only an explicit menu action opens the GUI.
//! The tray has no API client, credential cache or signing responsibilities.
use crate::{autostart, installation, storage::Store};
use anyhow::{Context, Result, ensure};
use ksni::blocking::TrayMethods;
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Sender},
    time::{Duration, Instant},
};

enum Action {
    Open,
    Autostart(bool),
    Exit,
}

struct Tray {
    actions: Sender<Action>,
    summary: String,
    autostart_enabled: bool,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "axia-remotedid".into()
    }
    fn title(&self) -> String {
        autostart::PRODUCT_NAME.into()
    }
    fn icon_name(&self) -> String {
        "dialog-password".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title(),
            description: self.summary.clone(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.actions.send(Action::Open);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};
        vec![
            StandardItem {
                label: "Abrir Axia RemotedID".into(),
                icon_name: "dialog-password".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.actions.send(Action::Open);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.summary.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            CheckmarkItem {
                label: "Iniciar com o sistema".into(),
                checked: self.autostart_enabled,
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray
                        .actions
                        .send(Action::Autostart(!tray.autostart_enabled));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.actions.send(Action::Exit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // GNOME can start its indicator extension after XDG autostart.
        // Wait for it silently and reconnect after shell/extension restarts.
        true
    }
}

fn lock(root: &Path) -> Result<Option<File>> {
    let directory = root.join("axia-remotedid");
    crate::storage::check_owned_private(&directory, true)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join("tray.lock"))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0
            && metadata.nlink() == 1,
        "Arquivo de controle da bandeja inválido"
    );
    // The kernel releases the lock on exit/crash. Never unlink the lock file;
    // another instance could otherwise lock a different inode.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(Some(file))
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error.into())
        }
    }
}

fn summary(store: &Store) -> String {
    if !store.path().join("wallet.json").exists() {
        return "Abra o aplicativo para conectar sua conta".into();
    }
    match store.load_wallet_public() {
        Ok(wallet) => format!("Certificados salvos: {}", wallet.len()),
        Err(_) => "Abra o aplicativo para verificar os certificados".into(),
    }
}

fn gui_command(store: &Store, root: &Path) -> Result<Command> {
    let launcher = Path::new("/usr/bin/axia-remotedid");
    // Preserve RPM frontend operations (including Firefox setup) for its
    // default state; custom state directories are handled by the native CLI.
    if store.path() == root.join("desktopid-linux") && installation::system_helper().is_some() {
        let mut command = Command::new(launcher);
        command.arg("gui");
        Ok(command)
    } else {
        let mut command = Command::new(autostart::executable()?);
        command.arg("--state-dir").arg(store.path()).arg("gui");
        Ok(command)
    }
}

/// A menu-triggered GUI is the only child with a window. Requests while it is
/// already open do not create more login dialogs.
pub fn run(store: &Store) -> Result<()> {
    let root = autostart::config_root()?;
    let Some(_lock) = lock(&root)? else {
        return Ok(());
    };
    let (actions, messages) = mpsc::channel();
    let handle = Tray {
        actions,
        summary: summary(store),
        autostart_enabled: autostart::enabled(&root)?,
    }
    .assume_sni_available(true)
    .spawn()
    .context("Bandeja indisponível nesta sessão")?;
    let mut window: Option<Child> = None;
    let mut refreshed = Instant::now();
    while !handle.is_closed() {
        if let Some(child) = window.as_mut()
            && child.try_wait()?.is_some()
        {
            window = None;
        }
        match messages.recv_timeout(Duration::from_millis(500)) {
            Ok(Action::Exit) => break,
            Ok(Action::Open) if window.is_none() => {
                match gui_command(store, &root)?
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                {
                    Ok(child) => window = Some(child),
                    Err(error) => eprintln!("Não foi possível abrir o aplicativo: {error}"),
                }
            }
            Ok(Action::Autostart(enable)) => {
                match autostart::configure(&root, &autostart::executable()?, store.path(), enable) {
                    Ok(()) => {
                        handle.update(|tray| tray.autostart_enabled = enable);
                    }
                    Err(error) => eprintln!("Não foi possível alterar a inicialização: {error}"),
                }
            }
            _ => {}
        }
        if refreshed.elapsed() >= Duration::from_secs(5) {
            let summary = summary(store);
            let enabled = autostart::enabled(&root)?;
            handle.update(|tray| {
                tray.summary = summary;
                tray.autostart_enabled = enabled;
            });
            refreshed = Instant::now();
        }
    }
    handle.shutdown().wait();
    Ok(())
}

/// Keep the user's existing first-run choice; do not ask again or initialize
/// account state when the GUI closes. Duplicate residents exit using flock.
pub fn start_background(store: &Store) -> Result<()> {
    if !autostart::enabled(&autostart::config_root()?)?
        || (std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none())
    {
        return Ok(());
    }
    enable_gnome_indicator();
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--state-dir")
        .arg(store.path())
        .arg("startup")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Não foi possível iniciar a bandeja")?;
    // Reap a duplicate resident promptly even while the GUI remains open.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A GUI autostart choice also enables installed GNOME indicator support.
/// This runs only on explicit GUI opening, never during session autostart.
/// Other desktops and enterprise restrictions are left to their SNI host.
fn enable_gnome_indicator() {
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("gnome"))
    });
    if !gnome || !Path::new("/usr/share/gnome-shell/extensions/appindicatorsupport@rgcjonas.gmail.com/metadata.json").is_file() { return; }
    use gtk::{gio, glib::variant::ToVariant};
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return;
    };
    let available = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        500,
        None::<&gio::Cancellable>,
    );
    if available.is_ok_and(|value| value.get::<(bool,)>() == Some((true,))) {
        return;
    }
    let _ = bus.call_sync(
        Some("org.gnome.Shell.Extensions"),
        "/org/gnome/Shell/Extensions",
        "org.gnome.Shell.Extensions",
        "EnableExtension",
        Some(&("appindicatorsupport@rgcjonas.gmail.com",).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        500,
        None::<&gio::Cancellable>,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resident_lock_rejects_duplicates_and_symlinks_and_recovers_after_exit() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("config");
        autostart::configure(
            &root,
            &temporary.path().join("binary"),
            &temporary.path().join("identity"),
            true,
        )
        .unwrap();
        let first = lock(&root).unwrap().unwrap();
        assert!(lock(&root).unwrap().is_none());
        drop(first);
        // Other parallel tests spawn processes. A fork can briefly inherit
        // this descriptor until exec closes O_CLOEXEC, retaining its flock.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if lock(&root).unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "resident lock was not released");
            std::thread::sleep(Duration::from_millis(5));
        }
        let path = root.join("axia-remotedid/tray.lock");
        std::fs::remove_file(&path).unwrap();
        let unrelated = temporary.path().join("unrelated");
        std::fs::write(&unrelated, b"preserve").unwrap();
        std::os::unix::fs::symlink(&unrelated, &path).unwrap();
        assert!(lock(&root).is_err());
        assert_eq!(std::fs::read(unrelated).unwrap(), b"preserve");
    }
    #[test]
    fn tray_menu_does_not_create_identity_or_request_authorization() {
        let temporary = tempfile::tempdir().unwrap();
        let state = temporary.path().join("identity");
        let store = Store::new(state.clone()).unwrap();
        let (actions, messages) = mpsc::channel();
        let mut tray = Tray {
            actions,
            summary: summary(&store),
            autostart_enabled: true,
        };
        assert!(!state.exists());
        let menu = ksni::Tray::menu(&tray);
        assert!(matches!(menu[1], ksni::MenuItem::Standard(ref item) if !item.enabled));
        if let ksni::MenuItem::Standard(item) = &menu[0] {
            (item.activate)(&mut tray);
        } else {
            panic!("Open item missing");
        }
        assert!(matches!(messages.recv().unwrap(), Action::Open));
        if let ksni::MenuItem::Checkmark(item) = &menu[3] {
            (item.activate)(&mut tray);
        } else {
            panic!("Autostart item missing");
        }
        assert!(matches!(messages.recv().unwrap(), Action::Autostart(false)));
        if let ksni::MenuItem::Standard(item) = &menu[4] {
            (item.activate)(&mut tray);
        } else {
            panic!("Exit item missing");
        }
        assert!(matches!(messages.recv().unwrap(), Action::Exit));
        assert!(!state.exists());
    }
}
