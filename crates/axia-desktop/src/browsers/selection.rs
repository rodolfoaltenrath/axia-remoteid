//! Separate profile enumeration from eligibility for existing-database setup.
use super::{FILES, Family, Target};
use std::{fs, io::ErrorKind};

pub(super) fn needs_initialization(target: &Target) -> bool {
    if target.family != Family::Firefox || !target.database.is_dir() {
        return false;
    }
    let mut missing = false;
    for name in FILES {
        // Never follow links or reinterpret inspection errors as empty profiles.
        // Those cases must still reach registration's existing safety checks.
        match fs::symlink_metadata(target.database.join(name)) {
            Ok(metadata) if metadata.is_file() => {}
            Err(error) if error.kind() == ErrorKind::NotFound => missing = true,
            _ => return false,
        }
    }
    missing
}

pub(super) fn eligible(targets: Vec<Target>, notices: &mut Vec<String>) -> Vec<Target> {
    targets.into_iter().filter(|target| {
        if needs_initialization(target) {
            notices.push(format!("{}: perfil ainda não preparado para certificados. Abra esse perfil no navegador uma vez e feche-o antes de configurar.", target.label));
            false
        } else { true }
    }).collect()
}

#[cfg(test)]
mod tests;
