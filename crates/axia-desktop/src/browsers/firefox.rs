//! Enumerate existing profiles separately from selecting one CLI default.
//! Default semantics: https://firefox-source-docs.mozilla.org/toolkit/profile/changes.html
use super::{BrowserInstallation, Family, FirefoxDefault, Target};
use anyhow::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Profile {
    name: Option<String>,
    installation_default: bool,
    profile_default: bool,
}

pub(super) struct Browser {
    pub id: &'static str,
    pub name: &'static str,
    pub commands: &'static [&'static str],
    pub desktop_ids: &'static [&'static str],
}

pub(super) const BROWSERS: &[Browser] = &[
    Browser {
        id: "firefox",
        name: "Firefox",
        commands: &["firefox"],
        desktop_ids: &["org.mozilla.firefox.desktop"],
    },
    Browser {
        id: "librewolf",
        name: "LibreWolf",
        commands: &["librewolf"],
        desktop_ids: &["io.gitlab.librewolf-community.desktop"],
    },
];

fn existing(path: &str, root: &Path, relative: bool) -> Result<Option<PathBuf>> {
    if path.trim().is_empty() || (!relative && !Path::new(path).is_absolute()) {
        return Ok(None);
    }
    let path = if relative {
        root.join(path)
    } else {
        PathBuf::from(path)
    };
    if !path.is_dir() {
        return Ok(None);
    }
    Ok(Some(path.canonicalize()?))
}

pub(super) fn targets(
    home: &Path,
    config: &Path,
    browser: &Browser,
    installation: &BrowserInstallation,
) -> Result<Vec<Target>> {
    let mut roots = BTreeSet::new();
    let mut profiles: BTreeMap<PathBuf, Profile> = BTreeMap::new();
    let profile_roots = if browser.id == "librewolf" {
        // LibreWolf documents the legacy root taking precedence whenever it
        // exists: https://librewolf.net/docs/faq/#how-do-i-migrate-to-the-new-xdg-paths
        let legacy = home.join(".librewolf");
        vec![if legacy.is_dir() {
            legacy
        } else {
            config.join("librewolf/librewolf")
        }]
    } else {
        vec![
            config.join("mozilla/firefox"),
            home.join(".mozilla/firefox"),
        ]
    };
    for root in profile_roots {
        if !root.join("profiles.ini").is_file() {
            continue;
        }
        let root = root.canonicalize()?;
        if !roots.insert(root.clone()) {
            continue;
        }
        let sections = crate::desktop::ini(&fs::read_to_string(root.join("profiles.ini"))?)?;
        for (section, values) in &sections {
            if !section.starts_with("Profile") {
                continue;
            }
            let Some(path) = values.get("path") else {
                continue;
            };
            let Some(path) = existing(
                path,
                &root,
                values.get("isrelative").is_none_or(|value| value == "1"),
            )?
            else {
                continue;
            };
            let profile = profiles.entry(path).or_default();
            if profile.name.is_none() {
                profile.name = values
                    .get("name")
                    .filter(|name| !name.trim().is_empty())
                    .cloned();
            }
            profile.profile_default |= values.get("default").is_some_and(|value| value == "1");
        }
        for (section, values) in &sections {
            if !section.starts_with("Install") {
                continue;
            }
            if let Some(path) = values.get("default")
                && let Some(path) = existing(path, &root, true)?
            {
                profiles.entry(path).or_default().installation_default = true;
            }
        }
    }
    let mut targets: Vec<_> = profiles
        .into_iter()
        .map(|(database, profile)| {
            let (firefox_default, suffix) = if profile.installation_default {
                (
                    Some(FirefoxDefault::Installation),
                    " (padrão de instalação)",
                )
            } else if profile.profile_default {
                (Some(FirefoxDefault::Profile), " (padrão geral)")
            } else {
                (None, "")
            };
            Target {
                family: Family::Firefox,
                label: format!(
                    "{} — {}{suffix}",
                    browser.name,
                    profile.name.as_deref().unwrap_or("Perfil Firefox")
                ),
                database,
                create: false,
                firefox_default,
                installations: vec![installation.clone()],
            }
        })
        .collect();
    // Keep defaults first for presentation, without dropping secondary profiles.
    targets.sort_by(|a, b| {
        let rank = |target: &Target| match target.firefox_default {
            Some(FirefoxDefault::Installation) => 0,
            Some(FirefoxDefault::Profile) => 1,
            None => 2,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.database.cmp(&b.database))
    });
    Ok(targets)
}

#[cfg(test)]
mod tests;
