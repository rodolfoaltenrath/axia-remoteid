//! Native browsers sharing the user's NSS database. Never executes a browser.
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    fs, io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};

pub(super) struct Browser {
    pub name: &'static str,
    pub commands: &'static [&'static str],
    executables: &'static [&'static str],
}

// Discovery and process detection must use the same command aliases. Internal
// ELF names complement them because wrappers can change argv[0] with exec -a.
pub(super) const BROWSERS: &[Browser] = &[
    Browser {
        name: "Chromium",
        commands: &["chromium", "chromium-browser"],
        executables: &["chrome"],
    },
    Browser {
        name: "Google Chrome",
        commands: &["google-chrome", "google-chrome-stable"],
        executables: &["chrome"],
    },
    Browser {
        name: "Brave",
        commands: &["brave-browser", "brave-browser-stable"],
        executables: &["brave"],
    },
    Browser {
        name: "Microsoft Edge",
        commands: &["microsoft-edge", "microsoft-edge-stable"],
        executables: &["msedge"],
    },
    Browser {
        name: "Vivaldi",
        commands: &["vivaldi", "vivaldi-stable"],
        executables: &["vivaldi-bin"],
    },
    Browser {
        name: "Opera",
        commands: &["opera"],
        executables: &[],
    },
];

fn known_name(path: &[u8]) -> bool {
    let name = Path::new(OsStr::from_bytes(path))
        .file_name()
        .unwrap_or_default()
        .as_bytes();
    BROWSERS.iter().any(|browser| {
        browser
            .commands
            .iter()
            .chain(browser.executables)
            .any(|candidate| candidate.as_bytes() == name)
    })
}

fn disappeared(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}

pub(super) fn running() -> Result<bool> {
    running_at(Path::new("/proc"), unsafe { libc::geteuid() }).context(
        "Não foi possível verificar os navegadores em execução; a configuração foi interrompida",
    )
}

fn running_at(proc: &Path, uid: u32) -> Result<bool> {
    for item in fs::read_dir(proc)? {
        let path = item?.path();
        if !path
            .file_name()
            .is_some_and(|name| !name.is_empty() && name.as_bytes().iter().all(u8::is_ascii_digit))
        {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if disappeared(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.uid() != uid {
            continue;
        }
        let executable = fs::read_link(path.join("exe"));
        if executable.as_ref().is_ok_and(|exe| {
            let bytes = exe.as_os_str().as_bytes();
            known_name(bytes.strip_suffix(b" (deleted)").unwrap_or(bytes))
        }) {
            return Ok(true);
        }
        let command = fs::read(path.join("cmdline"));
        if command.as_ref().is_ok_and(|bytes| {
            known_name(bytes.split(|byte| *byte == 0).next().unwrap_or_default())
        }) {
            return Ok(true);
        }
        // An exiting task may vanish between any two procfs operations. Empty
        // but readable cmdline is also normal for zombies and kernel threads.
        if command.is_err() && executable.is_err() {
            match fs::metadata(&path) {
                Err(error) if disappeared(&error) => continue,
                _ => bail!("Processo do usuário não pôde ser inspecionado"),
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
