//! Browser discovery and native NSS registration. No browser installation,
//! trust changes, password access, cloud requests or remote signatures.
//! Chromium DB selection follows docs/linux/cert_management.md (M146+).
use crate::desktop;
use anyhow::{Context, Result, ensure};
use libloading::Library;
use serde::Serialize;
use std::{
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

mod chromium;
mod discovery;
mod firefox;
pub mod report;
mod selection;
use report::{Blocked, FailureKind, Recovery, RecoveryState, SetupReport, SetupStatus};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirefoxDefault {
    Installation,
    Profile,
}

#[derive(Clone, Debug, Serialize)]
pub struct BrowserInstallation {
    pub id: &'static str,
    pub name: &'static str,
    pub executables: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub family: Family,
    pub label: String,
    pub database: PathBuf,
    pub create: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firefox_default: Option<FirefoxDefault>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub installations: Vec<BrowserInstallation>,
}

impl Target {
    fn selection_label(&self, home: &Path) -> String {
        if self.family != Family::Firefox {
            return self.label.clone();
        }
        let location = match self.database.strip_prefix(home) {
            Ok(relative) => format!("~/{}", relative.display()),
            Err(_) => self.database.display().to_string(),
        };
        format!("{}\n{location}", self.label)
    }
}

#[derive(Serialize)]
pub struct Inventory {
    pub installed: Vec<String>,
    pub targets: Vec<Target>,
    pub notices: Vec<String>,
    #[serde(skip)]
    ambiguous_firefox_profiles: bool,
}

impl Inventory {
    /// Discovery still lists every profile. Configuration offers only profiles
    /// whose existing SQL database files can be validated by register().
    pub fn configuration_targets(self, family: Option<Family>) -> (Vec<Target>, Vec<String>) {
        let mut notices = self.notices;
        let targets = self
            .targets
            .into_iter()
            .filter(|target| family.is_none_or(|family| target.family == family))
            .collect();
        let targets = selection::eligible(targets, &mut notices);
        (targets, notices)
    }

    /// Never guess among multiple installation defaults.
    pub fn firefox_target(self) -> Result<Target> {
        ensure!(
            !self.ambiguous_firefox_profiles,
            "Escolha um perfil Firefox explicitamente; há um perfil compartilhado com outro navegador"
        );
        let mut targets: Vec<_> = self
            .targets
            .into_iter()
            .filter(|target| {
                target.family == Family::Firefox
                    && target
                        .installations
                        .iter()
                        .any(|installation| installation.id == "firefox")
            })
            .collect();
        let default = [FirefoxDefault::Installation, FirefoxDefault::Profile]
            .into_iter()
            .find(|kind| {
                targets
                    .iter()
                    .any(|target| target.firefox_default == Some(*kind))
            });
        if let Some(default) = default {
            targets.retain(|target| target.firefox_default == Some(default));
        }
        ensure!(
            targets.len() == 1,
            "Escolha um perfil Firefox explicitamente"
        );
        Ok(targets.remove(0))
    }
}

fn absolute_environment(name: &str, default: PathBuf) -> Result<PathBuf> {
    let path = std::env::var_os(name).map(PathBuf::from).unwrap_or(default);
    ensure!(path.is_absolute(), "Diretório XDG deve ser absoluto");
    Ok(path)
}

#[cfg(test)]
fn discover_at(home: &Path, config: &Path, data: &Path, bin: &Path) -> Result<Inventory> {
    discover_in(
        home,
        config,
        data,
        &discovery::Search {
            bins: vec![bin.to_owned()],
            desktops: Vec::new(),
        },
    )
}

#[cfg(test)]
fn test_browser(path: &Path) {
    fs::write(path, b"\x7fELFsynthetic browser; never executed").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn discover_in(
    home: &Path,
    config: &Path,
    data: &Path,
    search: &discovery::Search,
) -> Result<Inventory> {
    let mut installed = Vec::new();
    let mut targets = Vec::new();
    let mut notices = Vec::new();
    for browser in firefox::BROWSERS {
        let Some(installation) = discovery::find(
            browser.id,
            browser.name,
            browser.commands,
            browser.desktop_ids,
            search,
            &mut notices,
        ) else {
            continue;
        };
        installed.push(browser.name.into());
        let profiles = firefox::targets(home, config, browser, &installation)?;
        if profiles.is_empty() {
            notices.push(format!(
                "Abra e feche o {} uma vez para preparar o perfil.",
                browser.name
            ));
        }
        targets.extend(profiles);
    }
    // A profile explicitly shared by products must not silently get the first
    // product's identity, nor be offered twice for the same physical DB.
    let mut owners = std::collections::BTreeMap::<PathBuf, std::collections::BTreeSet<&str>>::new();
    for target in &targets {
        for installation in &target.installations {
            owners
                .entry(target.database.clone())
                .or_default()
                .insert(installation.id);
        }
    }
    let ambiguous_firefox_profiles = owners
        .values()
        .any(|products| products.len() > 1 && products.contains("firefox"));
    let before = targets.len();
    targets.retain(|target| owners[&target.database].len() == 1);
    if targets.len() != before {
        notices.push("Um perfil compartilhado entre navegadores diferentes foi omitido para evitar uma associação ambígua. Use um perfil próprio de cada navegador.".into());
    }
    let chromium: Vec<_> = chromium::BROWSERS
        .iter()
        .filter_map(|browser| {
            discovery::find(
                browser.commands[0],
                browser.name,
                browser.commands,
                browser.desktop_ids,
                search,
                &mut notices,
            )
        })
        .collect();
    installed.extend(
        chromium
            .iter()
            .map(|installation| installation.name.to_owned()),
    );
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
            label: format!(
                "{} — certificados compartilhados",
                chromium
                    .iter()
                    .map(|installation| installation.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            database,
            create,
            firefox_default: None,
            installations: chromium,
        });
    }
    for (name, id) in [
        ("Firefox", "org.mozilla.firefox"),
        ("LibreWolf", "io.gitlab.librewolf-community"),
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
        ambiguous_firefox_profiles,
    })
}

pub fn discover(home: &Path) -> Result<Inventory> {
    let home = home.canonicalize()?;
    let config = absolute_environment("XDG_CONFIG_HOME", home.join(".config"))?;
    let data = absolute_environment("XDG_DATA_HOME", home.join(".local/share"))?;
    discover_in(
        &home,
        &config,
        &data,
        &discovery::Search::for_user(&home, &data),
    )
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

fn profile_lock(database: &Path, family: Family) -> Result<File> {
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
    // Firefox uses this exact fcntl lock. For Chromium this coordinates Axia
    // instances only: Chromium does not honor .axia-integration.lock.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &mut lock) } == -1 {
        let error = std::io::Error::last_os_error();
        return Err(
            if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EAGAIN)) {
                anyhow::Error::new(error).context(Blocked(FailureKind::ProfileBusy))
            } else {
                anyhow::Error::new(error)
                    .context("Não foi possível bloquear a base de certificados")
            },
        );
    }
    let legacy = database.join("lock");
    if family == Family::Firefox && legacy.is_symlink() {
        ensure!(
            fs::read_link(legacy)?
                .as_os_str()
                .as_bytes()
                .windows(2)
                .any(|part| part == b":+"),
            Blocked(FailureKind::ProfileBusy)
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

#[derive(Clone, Debug, Serialize)]
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
    register_with(target, module, home, migrate, chromium::running)
}

fn require_closed(family: Family, running: &mut impl FnMut() -> Result<bool>) -> Result<()> {
    if family == Family::Chromium {
        ensure!(
            !running().context(Blocked(FailureKind::InspectionUnavailable))?,
            Blocked(FailureKind::BrowserOpen)
        );
    }
    Ok(())
}

fn restore_closed(
    database: &Path,
    backup: &Path,
    family: Family,
    running: &mut impl FnMut() -> Result<bool>,
) -> Result<()> {
    if let Err(error) = require_closed(family, running) {
        let reason = error.downcast_ref::<Blocked>().map(|blocked| blocked.0);
        return Err(error.context(Recovery {
            state: RecoveryState::Suspended, backup_directory: backup.to_owned(), reason,
            detail: "Restauração automática suspensa: feche os navegadores e corrija eventuais erros de inspeção; o backup privado foi preservado".into(),
        }));
    }
    restore(database, backup).with_context(|| Recovery {
        state: RecoveryState::Failed,
        backup_directory: backup.to_owned(),
        reason: None,
        detail: "Não foi possível restaurar a configuração; o backup privado foi preservado".into(),
    })
}

fn register_with(
    target: &Target,
    module: &Path,
    home: &Path,
    migrate: bool,
    mut running: impl FnMut() -> Result<bool>,
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
    // Check before even creating a new shared database directory or lock file.
    require_closed(target.family, &mut running)?;
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
        ensure!(
            !selection::needs_initialization(target),
            Blocked(FailureKind::ProfileNotInitialized)
        );
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
    // Refuse before NSS_InitReadWrite: NSS initialization can write files too.
    // No restore is needed here because we have not opened NSS yet.
    if let Err(error) = require_closed(target.family, &mut running) {
        return Err(if let Some(backup) = &backup {
            error.context(Recovery {
                state: RecoveryState::NotNeeded,
                backup_directory: backup.clone(),
                reason: None,
                detail: "NSS não foi aberto nesta tentativa; a base anterior foi preservada".into(),
            })
        } else {
            error
        });
    }
    let operation = (|| {
        let mut nss = Database::open(&database)?;
        require_closed(target.family, &mut running)?;
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
            require_closed(target.family, &mut running)?;
            for name in FILES {
                fs::set_permissions(database.join(name), fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(count)
    })();
    let count = match operation {
        Ok(count) => count,
        Err(error) => {
            if let Some(ref backup) = backup {
                if let Err(restore_error) =
                    restore_closed(&database, backup, target.family, &mut running)
                {
                    // Keep both NSS and rollback causes, with a typed recovery
                    // context so suspended restoration is never called restored.
                    let mut recovery = restore_error
                        .downcast_ref::<Recovery>()
                        .cloned()
                        .expect("restore_closed always annotates recovery failures");
                    recovery.detail = format!("{restore_error:#}");
                    return Err(error.context(recovery));
                }
                return Err(
                    error.context(Recovery {
                        state: RecoveryState::Restored, backup_directory: backup.clone(), reason: None,
                        detail: "Configuração não concluída; a base anterior foi restaurada e o backup privado foi preservado".into(),
                    })
                );
            }
            return Err(error.context(
                "Configuração não concluída; feche os navegadores antes de tentar novamente",
            ));
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

#[cfg(test)]
#[path = "browsers/browser_guard_tests.rs"]
mod browser_guard_tests;

pub fn setup_ui(store: &crate::storage::Store, profile: Option<PathBuf>) -> Result<SetupReport> {
    desktop::user()?;
    crate::ui::init()?;
    setup_with(
        store,
        profile,
        || {
            let home = desktop::home()?;
            let inventory = discover(&home)?;
            Ok((home, inventory))
        },
        crate::ui::choose,
        |selected, home| {
            crate::ui::work(
                "Configurando os navegadores selecionados...",
                move |progress| {
                    let module = module(&home)?;
                    Ok(report::configure(selected, &module, &home, |target| {
                        // Progress delivery must not discard completed registrations.
                        let _ = progress.send(format!("Configurando {}...", target.label));
                    }))
                },
            )
        },
    )
}

// Keep the workflow under test while replacing only the graphical boundary.
// Tests never need a real profile, user session, module or cloud account.
fn setup_with(
    store: &crate::storage::Store,
    profile: Option<PathBuf>,
    prepare: impl FnOnce() -> Result<(PathBuf, Inventory)>,
    choose: impl FnOnce(&str, &[String]) -> Option<Vec<usize>>,
    configure: impl FnOnce(Vec<Target>, PathBuf) -> Result<SetupReport>,
) -> Result<SetupReport> {
    ensure!(
        store.load_wallet_public().is_ok(),
        "Conecte sua conta e atualize os certificados antes de configurar os navegadores"
    );
    let (home, mut inventory) = prepare()?;
    if let Some(database) = profile {
        inventory.targets = vec![Target {
            family: Family::Firefox,
            label: "Firefox".into(),
            database,
            create: false,
            firefox_default: None,
            installations: Vec::new(),
        }];
    }
    let (targets, notices) = inventory.configuration_targets(None);
    if targets.is_empty() {
        return Ok(SetupReport::empty(SetupStatus::NoTargets, notices));
    }
    let mut text = String::from(
        "Selecione os perfis em que deseja disponibilizar seus certificados. Um navegador pode ter mais de um perfil.\nFeche os navegadores antes de continuar. Nenhum navegador será instalado.",
    );
    for notice in &notices {
        text.push('\n');
        text.push_str(notice);
    }
    let choices: Vec<_> = targets
        .iter()
        .map(|target| target.selection_label(&home))
        .collect();
    let Some(selected) = choose(&text, &choices) else {
        return Ok(SetupReport::empty(SetupStatus::Cancelled, notices));
    };
    if selected.is_empty() {
        return Ok(SetupReport::empty(SetupStatus::Cancelled, notices));
    }
    let selected: Vec<_> = selected
        .into_iter()
        .map(|index| {
            targets
                .get(index)
                .cloned()
                .context("Seleção de navegador inválida")
        })
        .collect::<Result<_>>()?;
    let mut report = configure(selected, home)?;
    report.notices = notices;
    Ok(report)
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
        test_browser(&bin.join("firefox"));
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
            |_, _| panic!("must not register a browser"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Conecte sua conta"));
        assert!(!state.exists());
    }

    #[test]
    fn cancelled_empty_or_invalid_browser_selection_preserves_profile_and_never_registers() {
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
        let state_before: Vec<_> = fs::read_dir(store.path())
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let contents = fs::read(&path).unwrap();
                (path, contents)
            })
            .collect();
        for (selection, invalid) in [
            (None, false),
            (Some(vec![]), false),
            (Some(vec![0, 99]), true),
            (Some(vec![usize::MAX]), true),
        ] {
            let result = setup_with(
                &store,
                Some(profile.clone()),
                || {
                    Ok((
                        home.to_owned(),
                        Inventory {
                            installed: vec!["Firefox".into()],
                            targets: Vec::new(),
                            notices: Vec::new(),
                            ambiguous_firefox_profiles: false,
                        },
                    ))
                },
                |_, choices| {
                    assert_eq!(choices, &["Firefox\n~/profile"]);
                    selection
                },
                |_, _| panic!("cancelled, empty or invalid selection must not enter NSS"),
            );
            if invalid {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("Seleção de navegador inválida")
                );
            } else {
                assert_eq!(result.unwrap().status, SetupStatus::Cancelled);
            }
            for name in FILES {
                assert_eq!(
                    fs::read(profile.join(name)).unwrap(),
                    b"preserve laboratory profile"
                );
            }
            assert_eq!(fs::read_dir(&profile).unwrap().count(), FILES.len());
            assert_eq!(
                fs::read_dir(store.path()).unwrap().count(),
                state_before.len()
            );
            for (path, contents) in &state_before {
                assert_eq!(&fs::read(path).unwrap(), contents);
            }
            assert!(!home.join(".local/state").exists());
        }
        let report = setup_with(
            &store,
            None,
            || {
                Ok((
                    home.to_owned(),
                    Inventory {
                        installed: Vec::new(),
                        targets: Vec::new(),
                        notices: vec!["Nenhum navegador nativo encontrado".into()],
                        ambiguous_firefox_profiles: false,
                    },
                ))
            },
            |_, _| panic!("no targets must not show selection"),
            |_, _| panic!("no targets must not select a module or enter NSS"),
        )
        .unwrap();
        assert_eq!(report.status, SetupStatus::NoTargets);
        assert!(
            report
                .summary()
                .contains("Nenhum navegador nativo encontrado")
        );
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
        assert_eq!(
            error.downcast_ref::<Blocked>().unwrap().0,
            FailureKind::ProfileBusy
        );
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
            test_browser(&bin.join(name));
        }
        let firefox = config.join("mozilla/firefox");
        fs::create_dir_all(firefox.join("release")).unwrap();
        fs::create_dir(firefox.join("old")).unwrap();
        fs::write(firefox.join("profiles.ini"), "[Profile0]\nPath=old\nName=Old\nDefault=1\n[Profile1]\nPath=release\nName=Principal\n[InstallA]\nDefault=release\n").unwrap();
        fs::create_dir_all(data.join("pki/nssdb")).unwrap();
        let found = discover_at(home, &config, &data, &bin).unwrap();
        assert_eq!(found.installed.len(), 3);
        assert_eq!(found.targets.len(), 3);
        assert_eq!(found.targets[0].database, firefox.join("release"));
        assert_eq!(found.targets[1].database, firefox.join("old"));
        assert_eq!(found.targets[2].database, data.join("pki/nssdb"));
        assert_eq!(found.targets[2].installations.len(), 2);
        fs::create_dir_all(home.join(".pki/nssdb")).unwrap();
        assert_eq!(
            discover_at(home, &config, &data, &bin).unwrap().targets[2].database,
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
