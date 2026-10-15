//! Browser discovery and native NSS registration. No browser installation,
//! trust changes, password access, cloud requests or remote signatures.
//! Chromium DB selection follows docs/linux/cert_management.md (M146+).
use crate::desktop;
use anyhow::{Context, Result, ensure};
use libloading::Library;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    ffi::{CString, c_char, c_int, c_ulong, c_void},
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const MODULE_NAME: &str = "DesktopID Linux Alpha";
pub use crate::installation::runtime::SYSTEM_MODULE;
const FILES: [&str; 3] = ["pkcs11.txt", "cert9.db", "key4.db"];
const BACKUP_FILES: [&str; 9] = [
    "pkcs11.txt",
    "cert9.db",
    "key4.db",
    "cert9.db-wal",
    "cert9.db-shm",
    "cert9.db-journal",
    "key4.db-wal",
    "key4.db-shm",
    "key4.db-journal",
];
static NSS: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Firefox,
    Chromium,
}

#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub family: Family,
    pub label: String,
    pub database: PathBuf,
    pub create: bool,
}

#[derive(Serialize)]
pub struct Inventory {
    pub installed: Vec<String>,
    pub targets: Vec<Target>,
    pub notices: Vec<String>,
}

impl Inventory {
    /// Never guess among multiple installation defaults.
    pub fn firefox_target(self) -> Result<Target> {
        let mut targets = self
            .targets
            .into_iter()
            .filter(|target| target.family == Family::Firefox);
        let target = targets
            .next()
            .context("Escolha um perfil Firefox explicitamente")?;
        ensure!(
            targets.next().is_none(),
            "Escolha um perfil Firefox explicitamente"
        );
        Ok(target)
    }
}

fn absolute_environment(name: &str, default: PathBuf) -> Result<PathBuf> {
    let path = std::env::var_os(name).map(PathBuf::from).unwrap_or(default);
    ensure!(path.is_absolute(), "Diretório XDG deve ser absoluto");
    Ok(path)
}

fn profiles(home: &Path, config: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut preferred = Vec::new();
    let mut all = Vec::new();
    let mut roots = BTreeSet::new();
    for root in [
        config.join("mozilla/firefox"),
        home.join(".mozilla/firefox"),
    ] {
        if !roots.insert(root.clone()) || !root.join("profiles.ini").is_file() {
            continue;
        }
        let sections = desktop::ini(&fs::read_to_string(root.join("profiles.ini"))?)?;
        for (section, values) in &sections {
            if section.starts_with("Install") {
                if let Some(path) = values.get("default") {
                    let candidate = root.join(path);
                    if candidate.is_dir() {
                        preferred.push(candidate.canonicalize()?);
                    }
                }
            } else if section.starts_with("Profile")
                && let Some(path) = values.get("path")
            {
                let candidate = if values.get("isrelative").is_none_or(|value| value == "1") {
                    root.join(path)
                } else {
                    PathBuf::from(path)
                };
                if candidate.is_dir() {
                    let candidate = candidate.canonicalize()?;
                    if preferred.is_empty()
                        && values.get("default").is_some_and(|value| value == "1")
                    {
                        preferred.push(candidate.clone());
                    }
                    all.push((
                        candidate,
                        values
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| "Perfil Firefox".into()),
                    ));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    if !preferred.is_empty() {
        return Ok(preferred
            .into_iter()
            .filter(|path| seen.insert(path.clone()))
            .map(|path| {
                let label = all
                    .iter()
                    .find(|(candidate, _)| candidate == &path)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "Perfil Firefox".into());
                (path, label)
            })
            .collect());
    }
    Ok(all
        .into_iter()
        .filter(|(path, _)| seen.insert(path.clone()))
        .collect())
}

fn discover_at(home: &Path, config: &Path, data: &Path, bin: &Path) -> Result<Inventory> {
    let mut installed = Vec::new();
    let mut targets = Vec::new();
    let mut notices = Vec::new();
    if bin.join("firefox").is_file() {
        installed.push("Firefox".into());
        let profiles = profiles(home, config)?;
        if profiles.is_empty() {
            notices.push("Abra e feche o Firefox uma vez para preparar o perfil.".into());
        }
        targets.extend(profiles.into_iter().map(|(database, name)| Target {
            family: Family::Firefox,
            label: format!("Firefox — {name}"),
            database,
            create: false,
        }));
    }
    let chromium: Vec<&str> = [
        ("Chromium", &["chromium", "chromium-browser"][..]),
        (
            "Google Chrome",
            &["google-chrome", "google-chrome-stable"][..],
        ),
        ("Brave", &["brave-browser", "brave-browser-stable"][..]),
        (
            "Microsoft Edge",
            &["microsoft-edge", "microsoft-edge-stable"][..],
        ),
        ("Vivaldi", &["vivaldi", "vivaldi-stable"][..]),
        ("Opera", &["opera"][..]),
    ]
    .into_iter()
    .filter(|(_, commands)| commands.iter().any(|command| bin.join(command).is_file()))
    .map(|(name, _)| name)
    .collect();
    installed.extend(chromium.iter().map(|name| (*name).to_owned()));
    if !chromium.is_empty() {
        let legacy = home.join(".pki/nssdb");
        let current = data.join("pki/nssdb");
        // Existing legacy DB takes precedence even in Chromium M146+. A new
        // legacy DB is also recognized by older versions and these releases.
        let database = if legacy.is_dir() {
            legacy
        } else if current.is_dir() {
            current
        } else {
            legacy
        };
        let create = FILES
            .iter()
            .all(|name| !database.join(name).exists() && !database.join(name).is_symlink());
        targets.push(Target {
            family: Family::Chromium,
            label: format!("{} — certificados compartilhados", chromium.join(", ")),
            database,
            create,
        });
    }
    for (name, id) in [
        ("Firefox", "org.mozilla.firefox"),
        ("Chromium", "org.chromium.Chromium"),
        ("Google Chrome", "com.google.Chrome"),
        ("Brave", "com.brave.Browser"),
    ] {
        if home.join(".var/app").join(id).is_dir() {
            notices.push(format!("{name} em Flatpak: integração ainda não validada; o ambiente isolado usa configuração própria."));
        }
    }
    if installed.is_empty() {
        notices.push(
            "Nenhum navegador nativo compatível foi encontrado. O Axia não instala navegadores."
                .into(),
        );
    }
    Ok(Inventory {
        installed,
        targets,
        notices,
    })
}

pub fn discover(home: &Path) -> Result<Inventory> {
    let home = home.canonicalize()?;
    let config = absolute_environment("XDG_CONFIG_HOME", home.join(".config"))?;
    let data = absolute_environment("XDG_DATA_HOME", home.join(".local/share"))?;
    discover_at(&home, &config, &data, Path::new("/usr/bin"))
}

pub fn module(home: &Path) -> Result<PathBuf> {
    Ok(crate::installation::runtime::select(home)?.module)
}

pub fn registered_library(database: &Path) -> Result<Option<PathBuf>> {
    let path = database.join("pkcs11.txt");
    if !path.try_exists()? && !path.is_symlink() {
        return Ok(None);
    }
    desktop::owned_file(&path)?;
    let mut found = None;
    for section in fs::read_to_string(path)?
        .replace("\r\n", "\n")
        .split("\n\n")
    {
        let values: std::collections::BTreeMap<_, _> = section
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        if values.get("name") == Some(&MODULE_NAME) {
            ensure!(
                found.is_none(),
                "Há módulos com nome repetido; a configuração foi preservada"
            );
            found = Some(PathBuf::from(values.get("library").copied().unwrap_or("")));
        }
    }
    Ok(found)
}

fn chromium_running() -> Result<bool> {
    for item in fs::read_dir("/proc")? {
        let path = item?.path();
        if !path
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().iter().all(u8::is_ascii_digit))
            || !fs::metadata(&path)
                .is_ok_and(|metadata| metadata.uid() == unsafe { libc::geteuid() })
        {
            continue;
        }
        if let Ok(bytes) = fs::read(path.join("cmdline")) {
            let command = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
            let name = Path::new(std::ffi::OsStr::from_bytes(command))
                .file_name()
                .unwrap_or_default()
                .as_encoded_bytes();
            if matches!(
                name,
                b"chrome"
                    | b"chromium"
                    | b"chromium-browser"
                    | b"brave"
                    | b"brave-browser"
                    | b"msedge"
                    | b"microsoft-edge"
                    | b"vivaldi-bin"
                    | b"opera"
            ) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn profile_lock(database: &Path, family: Family) -> Result<File> {
    if family == Family::Chromium {
        ensure!(
            !chromium_running()?,
            "Feche os navegadores baseados em Chromium antes de configurar os certificados"
        );
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(database.join(if family == Family::Firefox {
            ".parentlock"
        } else {
            ".axia-integration.lock"
        }))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() } && metadata.nlink() == 1,
        "Arquivo de controle do navegador inválido"
    );
    let mut lock = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // Use Firefox's exact fcntl lock (flock is a different locking protocol).
    ensure!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &mut lock) } != -1,
        "Feche o navegador: a base de certificados está em uso"
    );
    let legacy = database.join("lock");
    if family == Family::Firefox && legacy.is_symlink() {
        ensure!(
            fs::read_link(legacy)?
                .as_os_str()
                .as_bytes()
                .windows(2)
                .any(|part| part == b":+"),
            "Feche o Firefox: há um bloqueio legado no perfil"
        );
    }
    Ok(file)
}

fn backup(database: &Path, home: &Path) -> Result<PathBuf> {
    let root = home.join(".local/state/desktopid-linux/browser-backups");
    desktop::directory(&root, home, true)?;
    let path = root.join(desktop::nonce()?);
    desktop::directory(&path, home, true)?;
    for name in BACKUP_FILES {
        let source = database.join(name);
        if source.try_exists()? || source.is_symlink() {
            desktop::owned_file(&source)?;
            crate::storage::write_new_private(&path.join(name), &fs::read(source)?)?;
        }
    }
    Ok(path)
}

fn restore(database: &Path, backup: &Path) -> Result<()> {
    for name in BACKUP_FILES {
        let original = backup.join(name);
        let destination = database.join(name);
        if original.exists() {
            desktop::replace(&destination, &fs::read(original)?, 0o600)?;
        } else if destination.exists() {
            desktop::owned_file(&destination)?;
            fs::remove_file(destination)?;
        }
    }
    Ok(())
}

type Status = unsafe extern "C" fn() -> c_int;
type Init = unsafe extern "C" fn(*const c_char) -> c_int;
type Add = unsafe extern "C" fn(*const c_char, *const c_char, c_ulong, c_ulong) -> c_int;
type Delete = unsafe extern "C" fn(*const c_char, *mut c_int) -> c_int;
type Find = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);
type List = unsafe extern "C" fn(*mut c_void) -> *mut Links;
#[repr(C)]
struct Links {
    next: *mut Links,
    previous: *mut Links,
}

/// NSS has process-global initialization state. The mutex covers init, module
/// operations and shutdown; function pointers never outlive their library.
struct Database {
    _library: Library,
    _guard: std::sync::MutexGuard<'static, ()>,
    shutdown: Status,
    error: Status,
    add: Add,
    delete: Delete,
    find: Find,
    destroy: Free,
    slot: Find,
    free_slot: Free,
    list: List,
    free_list: Free,
    active: bool,
}

impl Database {
    fn open(path: &Path) -> Result<Self> {
        let guard = NSS
            .lock()
            .map_err(|_| anyhow::anyhow!("Serviço de certificados indisponível"))?;
        // Load only the system NSS SONAME, never a path from browser metadata.
        let library = unsafe { Library::new("libnss3.so") }
            .context("Biblioteca NSS do sistema indisponível")?;
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                unsafe { *library.get::<$ty>(concat!($name, "\0").as_bytes())? }
            };
        }
        let init = symbol!("NSS_InitReadWrite", Init);
        let mut database = Self {
            shutdown: symbol!("NSS_Shutdown", Status),
            error: symbol!("PORT_GetError", Status),
            add: symbol!("SECMOD_AddNewModule", Add),
            delete: symbol!("SECMOD_DeleteModule", Delete),
            find: symbol!("SECMOD_FindModule", Find),
            destroy: symbol!("SECMOD_DestroyModule", Free),
            slot: symbol!("PK11_FindSlotByName", Find),
            free_slot: symbol!("PK11_FreeSlot", Free),
            list: symbol!("PK11_ListCertsInSlot", List),
            free_list: symbol!("CERT_DestroyCertList", Free),
            _library: library,
            _guard: guard,
            active: false,
        };
        let mut bytes = b"sql:".to_vec();
        bytes.extend_from_slice(path.as_os_str().as_bytes());
        let path = CString::new(bytes)?;
        database.check(
            unsafe { init(path.as_ptr()) },
            "Abrir a base de certificados",
        )?;
        database.active = true;
        Ok(database)
    }
    fn check(&self, status: c_int, operation: &str) -> Result<()> {
        ensure!(status == 0, "{operation} falhou (NSS {})", unsafe {
            (self.error)()
        });
        Ok(())
    }
    fn loaded(&self) -> bool {
        let module = unsafe { (self.find)(c"DesktopID Linux Alpha".as_ptr()) };
        if !module.is_null() {
            unsafe { (self.destroy)(module) };
        }
        !module.is_null()
    }
    fn add(&self, module: &Path) -> Result<()> {
        let path = CString::new(module.as_os_str().as_bytes())?;
        // Zero flags preserve the default provider and existing trust settings.
        self.check(
            unsafe { (self.add)(c"DesktopID Linux Alpha".as_ptr(), path.as_ptr(), 0, 0) },
            "Registrar o módulo",
        )?;
        ensure!(self.loaded(), "NSS não carregou o módulo registrado");
        Ok(())
    }
    fn remove(&self) -> Result<()> {
        let mut kind = 0;
        self.check(
            unsafe { (self.delete)(c"DesktopID Linux Alpha".as_ptr(), &mut kind) },
            "Migrar o módulo",
        )
    }
    fn count(&self) -> Result<usize> {
        let slot = unsafe { (self.slot)(c"Axia RemotedID".as_ptr()) };
        ensure!(
            !slot.is_null(),
            "NSS não encontrou o dispositivo Axia RemotedID"
        );
        let head = unsafe { (self.list)(slot) };
        let result = (|| {
            ensure!(
                !head.is_null(),
                "NSS não conseguiu consultar os certificados"
            );
            let mut node = unsafe { (*head).next };
            let mut count = 0;
            while node != head {
                ensure!(
                    !node.is_null() && count < 4096,
                    "Lista de certificados NSS inválida"
                );
                count += 1;
                node = unsafe { (*node).next };
            }
            Ok(count)
        })();
        if !head.is_null() {
            unsafe { (self.free_list)(head.cast()) };
        }
        unsafe { (self.free_slot)(slot) };
        result
    }
    fn close(&mut self) -> Result<()> {
        if self.active {
            self.check(
                unsafe { (self.shutdown)() },
                "Fechar a base de certificados",
            )?;
            self.active = false;
        }
        Ok(())
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { (self.shutdown)() };
        }
    }
}

#[derive(Serialize)]
pub struct Registration {
    pub browser: Family,
    pub profile: PathBuf,
    pub module: &'static str,
    pub registered_now: bool,
    pub migrated_legacy_module: bool,
    pub module_loaded_by_nss: bool,
    pub certificate_count: usize,
    pub backup_directory: Option<PathBuf>,
    pub remote_signature_requested: bool,
}

pub fn register(
    target: &Target,
    module: &Path,
    home: &Path,
    migrate: bool,
) -> Result<Registration> {
    let home = home.canonicalize()?;
    let system = module == Path::new(SYSTEM_MODULE);
    if system {
        ensure!(
            crate::installation::runtime::system_module() == Some(module),
            "Runtime do RPM antigo ou incompleto; atualize o RPM ou instale a versão nativa para este usuário"
        );
    }
    if !system {
        let metadata = desktop::owned_file(module)?;
        ensure!(
            metadata.mode() & 0o022 == 0 && module.canonicalize()?.starts_with(&home),
            "Biblioteca deve ser uma instalação protegida do usuário"
        );
    }
    ensure!(
        !module
            .as_os_str()
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b'\n' | b'\r' | 0)),
        "Caminho do módulo inválido"
    );
    if target.create {
        ensure!(
            target.family == Family::Chromium
                && FILES.iter().all(|name| !target.database.join(name).exists()
                    && !target.database.join(name).is_symlink()),
            "Base existente foi preservada"
        );
        desktop::directory(&target.database, &home, true)?;
    }
    let database = target.database.canonicalize()?;
    desktop::directory(&database, &home, false)?;
    if !target.create {
        for name in FILES {
            let metadata = desktop::owned_file(&database.join(name))?;
            ensure!(
                metadata.mode() & 0o022 == 0 && metadata.nlink() == 1,
                "Arquivo de certificados possui permissões ou vínculos inseguros"
            );
        }
    }
    let _lock = profile_lock(&database, target.family)?;
    let existing = registered_library(&database)?;
    let legacy = home.join(".local/lib/desktopid-linux/libdesktopid_pkcs11.so");
    let from_user = system && existing.as_ref() == Some(&legacy);
    let from_old_system = existing.as_deref().is_some_and(|existing| {
        crate::installation::runtime::can_replace_old_system(&home, existing, module)
    });
    let migrated = migrate && (from_user || from_old_system);
    if migrated && from_user {
        ensure!(
            desktop::owned_file(&legacy)?.mode() & 0o022 == 0,
            "Módulo anterior possui permissões inseguras; foi preservado"
        );
    }
    ensure!(
        existing.is_none() || existing.as_deref() == Some(module) || migrated,
        "Outro módulo utiliza esse nome; a configuração existente foi preservada"
    );
    let backup = if existing.is_none() || migrated {
        Some(backup(&database, &home)?)
    } else {
        None
    };
    let operation = (|| {
        let mut nss = Database::open(&database)?;
        if target.family == Family::Chromium {
            ensure!(
                !chromium_running()?,
                "Feche os navegadores baseados em Chromium antes de configurar os certificados"
            );
        }
        if migrated {
            nss.remove()?;
            nss.add(module)?;
        } else if existing.is_none() {
            nss.add(module)?;
        } else {
            ensure!(nss.loaded(), "O módulo configurado não pôde ser carregado");
        }
        let count = nss.count()?;
        nss.close()?;
        ensure!(
            registered_library(&database)?.as_deref() == Some(module),
            "NSS não salvou o caminho do módulo"
        );
        if target.create {
            for name in FILES {
                fs::set_permissions(database.join(name), fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(count)
    })();
    let count = match operation {
        Ok(count) => count,
        Err(error) => {
            if let Some(ref backup) = backup
                && restore(&database, backup).is_err()
            {
                return Err(error.context(
                    "Não foi possível restaurar a configuração; o backup privado foi preservado",
                ));
            }
            return Err(error.context("Configuração não concluída; a base anterior foi preservada"));
        }
    };
    Ok(Registration {
        browser: target.family,
        profile: database,
        module: MODULE_NAME,
        registered_now: existing.is_none(),
        migrated_legacy_module: migrated,
        module_loaded_by_nss: true,
        certificate_count: count,
        backup_directory: backup,
        remote_signature_requested: false,
    })
}

pub fn setup_ui(store: &crate::storage::Store, profile: Option<PathBuf>) -> Result<()> {
    desktop::user()?;
    crate::ui::init()?;
    let message = setup_with(
        store,
        profile,
        || {
            let home = desktop::home()?;
            let module = module(&home)?;
            let inventory = discover(&home)?;
            Ok((home, module, inventory))
        },
        crate::ui::choose,
        |selected, module, home| {
            crate::ui::work(
                "Configurando os navegadores selecionados...",
                move |progress| {
                    let mut results = Vec::new();
                    for target in selected {
                        progress
                            .send(format!("Configurando {}...", target.label))
                            .context("Interface indisponível")?;
                        let result = register(&target, &module, &home, true);
                        results.push(match result {
                            Ok(report) => format!(
                                "{}: configurado; {} certificado(s) disponível(is).",
                                target.label, report.certificate_count
                            ),
                            Err(error) => format!("{}: {}", target.label, error),
                        });
                    }
                    Ok(results)
                },
            )
        },
    )?;
    if let Some(message) = message {
        crate::ui::message(&message);
    }
    Ok(())
}

// Keep the workflow under test while replacing only the graphical boundary.
// Tests never need a real profile, user session, module or cloud account.
fn setup_with(
    store: &crate::storage::Store,
    profile: Option<PathBuf>,
    prepare: impl FnOnce() -> Result<(PathBuf, PathBuf, Inventory)>,
    choose: impl FnOnce(&str, &[String]) -> Option<Vec<usize>>,
    configure: impl FnOnce(Vec<Target>, PathBuf, PathBuf) -> Result<Vec<String>>,
) -> Result<Option<String>> {
    ensure!(
        store.load_wallet_public().is_ok(),
        "Conecte sua conta e atualize os certificados antes de configurar os navegadores"
    );
    let (home, module, inventory) = prepare()?;
    let targets = if let Some(database) = profile {
        vec![Target {
            family: Family::Firefox,
            label: "Firefox".into(),
            database,
            create: false,
        }]
    } else {
        inventory.targets
    };
    if targets.is_empty() {
        return Ok(Some(inventory.notices.join("\n")));
    }
    let mut text = String::from(
        "Feche os navegadores antes de continuar. Selecione onde deseja disponibilizar seus certificados.\nO Axia configura os navegadores instalados; nenhum navegador será instalado.",
    );
    for notice in inventory.notices {
        text.push('\n');
        text.push_str(&notice);
    }
    let choices: Vec<_> = targets.iter().map(|target| target.label.clone()).collect();
    let Some(selected) = choose(&text, &choices) else {
        return Ok(None);
    };
    let selected: Vec<_> = selected
        .into_iter()
        .map(|index| targets[index].clone())
        .collect();
    let results = configure(selected, module, home)?;
    Ok(Some(format!(
        "{}\n\nReabra os navegadores configurados e escolha a entrada com certificado digital no serviço desejado.",
        results.join("\n\n")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_firefox_install_defaults_require_explicit_selection() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path();
        let config = home.join("config");
        let root = config.join("mozilla/firefox");
        let bin = home.join("bin");
        fs::create_dir_all(root.join("first")).unwrap();
        fs::create_dir(root.join("second")).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("firefox"), b"laboratory browser").unwrap();
        fs::write(
            root.join("profiles.ini"),
            "[InstallA]\nDefault=first\n[InstallB]\nDefault=second\n",
        )
        .unwrap();
        let inventory = discover_at(home, &config, &home.join("data"), &bin).unwrap();
        assert_eq!(inventory.targets.len(), 2);
        assert!(
            inventory
                .firefox_target()
                .unwrap_err()
                .to_string()
                .contains("explicitamente")
        );
    }

    #[test]
    fn browser_setup_without_account_never_prepares_or_registers_a_profile() {
        let temporary = tempfile::tempdir().unwrap();
        let state = temporary.path().join("must-not-exist");
        let store = crate::storage::Store::new(state.clone()).unwrap();
        let error = setup_with(
            &store,
            None,
            || panic!("must not inspect a browser without an account"),
            |_, _| panic!("must not ask to register a browser"),
            |_, _, _| panic!("must not register a browser"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Conecte sua conta"));
        assert!(!state.exists());
    }

    #[test]
    fn cancelled_browser_setup_preserves_profile_and_never_registers() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path();
        let store = crate::storage::Store::new(home.join("identity")).unwrap();
        let mut config = store
            .initialize("https://example.invalid", "laboratory")
            .unwrap();
        config.desktop_code = Some("lab-code".into());
        store.save_registration(&config).unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../desktopid-client/tests/fixtures/python-vectors.json"
        ))
        .unwrap();
        store
            .save_wallet(&[crate::client::WalletCertificate {
                key_name: "LAB-SERIAL;LAB-ISSUER".into(),
                certificate: openssl::x509::X509::from_pem(
                    fixture["certificate_pem"].as_str().unwrap().as_bytes(),
                )
                .unwrap(),
            }])
            .unwrap();
        let profile = home.join("profile");
        fs::create_dir(&profile).unwrap();
        for name in FILES {
            fs::write(profile.join(name), b"preserve laboratory profile").unwrap();
        }
        let identity = fs::read(store.path().join("identity.pem")).unwrap();
        let wallet = fs::read(store.path().join("wallet.json")).unwrap();
        let result = setup_with(
            &store,
            Some(profile.clone()),
            || {
                Ok((
                    home.to_owned(),
                    home.join("unused-module.so"),
                    Inventory {
                        installed: vec!["Firefox".into()],
                        targets: Vec::new(),
                        notices: Vec::new(),
                    },
                ))
            },
            |_, choices| {
                assert_eq!(choices, &["Firefox"]);
                None
            },
            |_, _, _| panic!("cancel must not enter NSS or register a module"),
        )
        .unwrap();
        assert!(result.is_none());
        for name in FILES {
            assert_eq!(
                fs::read(profile.join(name)).unwrap(),
                b"preserve laboratory profile"
            );
        }
        assert_eq!(
            fs::read(store.path().join("identity.pem")).unwrap(),
            identity
        );
        assert_eq!(fs::read(store.path().join("wallet.json")).unwrap(), wallet);
        assert!(!home.join(".local/state").exists());
    }

    #[test]
    fn firefox_profile_lock_held_by_another_process_is_preserved() {
        use std::io::{BufRead, Read, Write};
        use std::process::{Command, Stdio};
        const CHILD_PROFILE: &str = "AXIA_TEST_FIREFOX_LOCK_PROFILE";
        if let Some(profile) = std::env::var_os(CHILD_PROFILE) {
            // Independent browser-style fcntl lock, not the production helper.
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(Path::new(&profile).join(".parentlock"))
                .unwrap();
            let mut lock = libc::flock {
                l_type: libc::F_WRLCK as _,
                l_whence: libc::SEEK_SET as _,
                l_start: 0,
                l_len: 0,
                l_pid: 0,
            };
            assert_ne!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &mut lock) },
                -1
            );
            println!("AXIA_FIREFOX_LOCK_READY");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_exact(&mut [0]).unwrap();
            drop(file);
            return;
        }
        struct Holder(std::process::Child);
        impl Drop for Holder {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let temporary = tempfile::tempdir().unwrap();
        let profile = temporary.path();
        let mut child = Holder(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "browsers::tests::firefox_profile_lock_held_by_another_process_is_preserved",
                    "--nocapture",
                ])
                .env(CHILD_PROFILE, profile)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let output = child.0.stdout.take().unwrap();
        let (ready, receiver) = std::sync::mpsc::channel();
        let output_reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(output)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if line.ends_with("AXIA_FIREFOX_LOCK_READY") {
                    let _ = ready.send(());
                }
            }
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let error = profile_lock(profile, Family::Firefox).unwrap_err();
        assert!(error.to_string().contains("em uso"));
        assert!(child.0.try_wait().unwrap().is_none());
        child.0.stdin.take().unwrap().write_all(b"x").unwrap();
        assert!(child.0.wait().unwrap().success());
        output_reader.join().unwrap();
        let _lock = profile_lock(profile, Family::Firefox).unwrap();
        assert!(profile.join(".parentlock").is_file());
    }
    #[test]
    fn discovery_detects_both_families_and_respects_existing_chromium_database() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path();
        let config = home.join("config");
        let data = home.join("data");
        let bin = home.join("bin");
        fs::create_dir(&bin).unwrap();
        for name in ["firefox", "google-chrome", "brave-browser"] {
            fs::write(bin.join(name), b"test").unwrap();
        }
        let firefox = config.join("mozilla/firefox");
        fs::create_dir_all(firefox.join("release")).unwrap();
        fs::create_dir(firefox.join("old")).unwrap();
        fs::write(firefox.join("profiles.ini"), "[Profile0]\nPath=old\nName=Old\nDefault=1\n[Profile1]\nPath=release\nName=Principal\n[InstallA]\nDefault=release\n").unwrap();
        fs::create_dir_all(data.join("pki/nssdb")).unwrap();
        let found = discover_at(home, &config, &data, &bin).unwrap();
        assert_eq!(found.installed.len(), 3);
        assert_eq!(found.targets.len(), 2);
        assert_eq!(found.targets[0].database, firefox.join("release"));
        assert_eq!(found.targets[1].database, data.join("pki/nssdb"));
        fs::create_dir_all(home.join(".pki/nssdb")).unwrap();
        assert_eq!(
            discover_at(home, &config, &data, &bin).unwrap().targets[1].database,
            home.join(".pki/nssdb")
        );
    }
    #[test]
    fn browser_backup_is_private_and_restore_removes_only_new_database_files() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path();
        let profile = home.join("profile");
        fs::create_dir(&profile).unwrap();
        fs::write(profile.join("pkcs11.txt"), b"preserve").unwrap();
        let saved = backup(&profile, home).unwrap();
        assert_eq!(fs::metadata(&saved).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(saved.join("pkcs11.txt")).unwrap().mode() & 0o777,
            0o600
        );
        fs::write(profile.join("pkcs11.txt"), b"changed").unwrap();
        fs::write(profile.join("cert9.db"), b"new").unwrap();
        fs::write(profile.join("unrelated"), b"keep").unwrap();
        restore(&profile, &saved).unwrap();
        assert_eq!(fs::read(profile.join("pkcs11.txt")).unwrap(), b"preserve");
        assert!(!profile.join("cert9.db").exists());
        assert!(profile.join("unrelated").exists());
    }
    #[test]
    fn profile_lock_refuses_symlinks_without_touching_the_target() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = temporary.path();
        fs::write(profile.join("unrelated"), b"keep").unwrap();
        std::os::unix::fs::symlink(profile.join("unrelated"), profile.join(".parentlock")).unwrap();
        assert!(profile_lock(profile, Family::Firefox).is_err());
        assert_eq!(fs::read(profile.join("unrelated")).unwrap(), b"keep");
    }
}
