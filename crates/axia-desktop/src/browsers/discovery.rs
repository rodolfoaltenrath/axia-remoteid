//! Read-only discovery of declared native launchers. Never executes Exec,
//! TryExec, a shell, a browser, or an application metadata helper.
use super::BrowserInstallation;
use std::{
    collections::BTreeSet,
    env,
    ffi::CString,
    fs,
    io::{Read, Take},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

pub(super) struct Search {
    pub bins: Vec<PathBuf>,
    pub desktops: Vec<PathBuf>,
}

impl Search {
    pub fn for_user(home: &Path, data: &Path) -> Self {
        let mut bins: Vec<_> = env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default();
        bins.extend([
            home.join(".local/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ]);
        let mut desktops = vec![data.join("applications")];
        let data_dirs = env::var_os("XDG_DATA_DIRS")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        desktops.extend(
            env::split_paths(&data_dirs)
                .filter(|path| path.is_absolute())
                .map(|path| path.join("applications")),
        );
        desktops.extend([
            home.join(".local/share/flatpak/exports/share/applications"),
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        ]);
        Self {
            bins: absolute_unique(bins),
            desktops: absolute_unique(desktops),
        }
    }
}

fn absolute_unique(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|path| path.is_absolute() && seen.insert(path.clone()))
        .collect()
}

fn reader(path: &Path) -> std::io::Result<Take<fs::File>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("non-regular launcher"));
    }
    Ok(file.take(65536))
}

fn text(path: &Path) -> Option<String> {
    let path = path.canonicalize().ok()?;
    let mut reader = reader(&path).ok()?;
    if reader.get_ref().metadata().ok()?.len() > 65536 {
        return None;
    }
    let mut text = String::new();
    reader.read_to_string(&mut text).ok()?;
    Some(text)
}

fn sandbox_path(path: &Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        matches!(name.as_ref(), "flatpak" | ".var" | "snap") || name.starts_with(".mount_")
    }) || path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("AppImage"))
}

fn executable(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Check availability for this process, including ACLs; do not run it.
    unsafe { libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::X_OK, libc::AT_EACCESS) == 0 }
}

fn product(name: &str) -> Option<&'static str> {
    super::firefox::BROWSERS
        .iter()
        .find(|browser| browser.name.eq_ignore_ascii_case(name) || browser.commands.contains(&name))
        .map(|browser| browser.id)
}

fn app_product(path: &Path) -> Option<String> {
    let source = text(&path.parent()?.join("application.ini"))?;
    let mut in_app = false;
    let mut name = None;
    for line in source.lines().map(str::trim) {
        if line.starts_with('[') {
            in_app = line.eq_ignore_ascii_case("[App]");
        }
        if in_app && let Some((key, value)) = line.split_once('=') {
            if key.eq_ignore_ascii_case("RemotingName") {
                return Some(value.trim().to_ascii_lowercase());
            }
            if key.eq_ignore_ascii_case("Name") {
                name = Some(value.trim().to_ascii_lowercase());
            }
        }
    }
    name
}

fn native(path: &Path, id: &str) -> Result<PathBuf, &'static str> {
    let canonical = path.canonicalize().map_err(|_| "lançador indisponível")?;
    let metadata = fs::metadata(&canonical).map_err(|_| "lançador indisponível")?;
    if !metadata.is_file() || metadata.mode() & 0o111 == 0 || !executable(&canonical) {
        return Err("lançador sem permissão de execução");
    }
    if sandbox_path(&canonical) {
        return Err("Flatpak, Snap ou AppImage: integração não validada");
    }
    if super::chromium::BROWSERS
        .iter()
        .any(|browser| browser.commands[0] == id)
        && !super::chromium::known_name(canonical.as_os_str().as_bytes())
    {
        return Err("nome do executável não coberto pela proteção Chromium");
    }
    let mut data = Vec::new();
    reader(&canonical)
        .map_err(|_| "lançador ilegível")?
        .read_to_end(&mut data)
        .map_err(|_| "lançador ilegível")?;
    if data
        .get(8..11)
        .is_some_and(|magic| magic == b"AI\x01" || magic == b"AI\x02")
    {
        return Err("AppImage: integração não validada");
    }
    if data.starts_with(b"#!") {
        if metadata.len() > data.len() as u64 {
            return Err("lançador grande demais para análise estática");
        }
        let script = String::from_utf8_lossy(&data);
        if script.contains("flatpak")
            || script.contains("/snap/")
            || script.contains("snap run")
            || script.contains("AppImage")
        {
            return Err("lançador de ambiente isolado: integração não validada");
        }
        for line in script.lines().map(str::trim) {
            if let Some(exec) = line.strip_prefix("exec ")
                && let Ok(words) = gtk::glib::shell_parse_argv(exec)
                && let Some(other) = words
                    .first()
                    .and_then(|word| Path::new(word).file_name())
                    .and_then(|word| word.to_str())
                    .and_then(product)
                && other != id
            {
                return Err("lançador encaminha para outro navegador");
            }
        }
    } else if !data.starts_with(b"\x7fELF") {
        return Err("formato do lançador não reconhecido");
    }
    let declared = app_product(&canonical);
    let named = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(product);
    if declared
        .as_deref()
        .or(named)
        .is_some_and(|other| other != id)
    {
        return Err("lançador pertence a outro navegador");
    }
    Ok(canonical)
}

fn resolve(command: &str, search: &Search) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return Some(path.to_owned());
    }
    if path.components().count() != 1 {
        return None;
    }
    search
        .bins
        .iter()
        .map(|directory| directory.join(path))
        .find(|path| {
            executable(path)
                && fs::metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0)
        })
}

fn desktop_command(exec: &str) -> Option<String> {
    // GLib parses quotation only. There is no process or shell invocation.
    let words = gtk::glib::shell_parse_argv(exec).ok()?;
    let words: Vec<_> = words
        .iter()
        .map(|word| word.to_str())
        .collect::<Option<_>>()?;
    let mut at = 0;
    if matches!(*words.first()?, "env" | "/usr/bin/env" | "/bin/env") {
        at += 1;
        while let Some((key, _)) = words.get(at).and_then(|word| word.split_once('=')) {
            if !matches!(key, "GDK_BACKEND" | "MOZ_ENABLE_WAYLAND") {
                return None;
            }
            at += 1;
        }
        if words.get(at) == Some(&"--") {
            at += 1;
        }
    }
    let command = *words.get(at)?;
    if command.contains(['%', '=', '$', '`']) {
        return None;
    }
    if words[at + 1..].iter().any(|word| {
        word.contains('%')
            && !matches!(*word, "%u" | "%U" | "%f" | "%F" | "%i" | "%c" | "%k" | "%%")
    }) {
        return None;
    }
    if words[at + 1..].iter().any(|word| {
        matches!(
            *word,
            "-P" | "-p"
                | "--profile"
                | "-profile"
                | "--ProfileManager"
                | "--profilemanager"
                | "--user-data-dir"
                | "-app"
                | "--app"
        ) || word.starts_with("--profile=")
            || word.starts_with("-P")
            || word.starts_with("--user-data-dir=")
    }) {
        return None;
    }
    Some(command.to_owned())
}

pub(super) fn find(
    id: &'static str,
    name: &'static str,
    commands: &[&str],
    desktop_ids: &[&str],
    search: &Search,
    notices: &mut Vec<String>,
) -> Option<BrowserInstallation> {
    let mut executables = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    for directory in &search.bins {
        for command in commands {
            add(directory.join(command), id, &mut executables, &mut reasons);
        }
    }
    let ids: BTreeSet<_> = commands
        .iter()
        .map(|command| format!("{command}.desktop"))
        .chain(desktop_ids.iter().map(|id| (*id).to_owned()))
        .collect();
    // Respect a user's override (including Hidden=true) of the same desktop ID.
    let mut seen_ids = BTreeSet::new();
    for directory in &search.desktops {
        for desktop_id in &ids {
            let path = directory.join(desktop_id);
            if !path.exists() || !seen_ids.insert(desktop_id) {
                continue;
            }
            let Some(source) = text(&path) else {
                continue;
            };
            let Ok(sections) = crate::desktop::ini(&source) else {
                continue;
            };
            let Some(entry) = sections.get("Desktop Entry") else {
                continue;
            };
            if entry.get("hidden").is_some_and(|value| value == "true")
                || entry.get("type").is_none_or(|value| value != "Application")
            {
                continue;
            }
            if entry.contains_key("x-flatpak") || entry.contains_key("x-snapinstancename") {
                reasons.insert("Flatpak ou Snap: integração não validada");
                continue;
            }
            let Some(exec) = entry.get("exec") else {
                continue;
            };
            let Some(command) = desktop_command(exec) else {
                reasons.insert("lançador com parâmetros não suportados");
                continue;
            };
            let basename = Path::new(&command)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if matches!(basename, "flatpak" | "snap" | "AppRun") {
                reasons.insert("Flatpak, Snap ou AppImage: integração não validada");
                continue;
            }
            // Recognized desktop ID alone cannot label an arbitrary command.
            if !commands.contains(&basename) {
                reasons.insert("executável não corresponde ao navegador");
                continue;
            }
            if entry.get("tryexec").is_some_and(|command| {
                resolve(command, search)
                    .and_then(|path| native(&path, id).ok())
                    .is_none()
            }) {
                reasons.insert("TryExec indisponível ou incompatível");
                continue;
            }
            if let Some(path) = resolve(&command, search) {
                add(path, id, &mut executables, &mut reasons);
            } else {
                reasons.insert("lançador indisponível");
            }
        }
    }
    notices.extend(
        reasons
            .into_iter()
            .map(|reason| format!("{name}: {reason}.")),
    );
    (!executables.is_empty()).then(|| BrowserInstallation {
        id,
        name,
        executables: executables.into_iter().collect(),
    })
}

fn add(
    path: PathBuf,
    id: &str,
    executables: &mut BTreeSet<PathBuf>,
    reasons: &mut BTreeSet<&'static str>,
) {
    if !path.exists() && !path.is_symlink() {
        return;
    }
    match native(&path, id) {
        Ok(path) => {
            executables.insert(path);
        }
        Err(reason) => {
            reasons.insert(reason);
        }
    }
}

#[cfg(test)]
mod tests;
