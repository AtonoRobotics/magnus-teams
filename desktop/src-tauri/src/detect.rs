//! Which supported programs are installed here, with their versions and
//! paths (connectors spec, Detect): on Linux the package records, Flatpak,
//! Snap, the PATH and install paths; on Windows the uninstall keys and
//! install paths. What is read of the computer goes through `Machine` and
//! `Registry`, so the detectors are tested with fakes on any system.
// Each system's detector is built and tested everywhere; only its own runs.
#![cfg_attr(windows, allow(dead_code))]

use crate::programs::{Program, PROGRAMS};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One supported program found here.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub program: &'static str,
    pub version: Option<String>,
    pub path: Option<String>,
}

pub trait LocalPrograms: Send + Sync {
    fn detect(&self) -> Vec<Found>;
}

/// What detection reads of the computer. Paths are written with the
/// system's own separator.
pub trait Machine: Send + Sync {
    fn exists(&self, path: &str) -> bool;
    /// The names of the entries in a folder.
    fn entries(&self, path: &str) -> Vec<String>;
    fn read(&self, path: &str) -> Option<String>;
    /// What a listing program prints (`flatpak list`, `snap list`), if it runs.
    fn listing(&self, program: &str, args: &[&str]) -> Option<String>;
    fn path_dirs(&self) -> Vec<String>;
    /// `LOCALAPPDATA` or `PROGRAMFILES`.
    fn folder(&self, name: &str) -> Option<String>;
}

/// One uninstall entry under `...\CurrentVersion\Uninstall`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uninstall {
    pub name: String,
    pub version: Option<String>,
    pub location: Option<String>,
}

pub trait Registry: Send + Sync {
    fn uninstall_entries(&self) -> Vec<Uninstall>;
}

const DPKG_STATUS: &str = "/var/lib/dpkg/status";

/// The first path a pattern names that exists. A component ending in `*`
/// matches the entries that start with the rest, tried newest first (the
/// last by name, as these programs name their versioned folders).
pub fn resolve(m: &dyn Machine, pattern: &str, sep: char) -> Option<String> {
    let parts: Vec<&str> = pattern.split(sep).collect();
    let (first, rest) = parts.split_first()?;
    walk(m, first.to_string(), rest, sep)
}

fn walk(m: &dyn Machine, at: String, rest: &[&str], sep: char) -> Option<String> {
    let Some((part, rest)) = rest.split_first() else { return m.exists(&at).then_some(at) };
    match part.strip_suffix('*') {
        Some(prefix) => {
            let mut names: Vec<String> = m.entries(&at).into_iter().filter(|n| n.starts_with(prefix)).collect();
            names.sort();
            names.into_iter().rev().find_map(|n| walk(m, format!("{at}{sep}{n}"), rest, sep))
        }
        None => walk(m, format!("{at}{sep}{part}"), rest, sep),
    }
}

/// Installed packages and their versions, from dpkg's status file.
pub fn dpkg_versions(status: &str) -> Vec<(String, String)> {
    status
        .split("\n\n")
        .filter_map(|block| {
            let field = |name: &str| block.lines().find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_string()));
            let installed = field("Status:").is_some_and(|s| s.ends_with(" installed"));
            Some((field("Package:")?, field("Version:")?)).filter(|_| installed)
        })
        .collect()
}

/// `flatpak list --app --columns=application,version`: one id and version per line.
pub fn flatpak_versions(list: &str) -> Vec<(String, Option<String>)> {
    list.lines().filter_map(|l| {
        let mut cols = l.split('\t').map(str::trim);
        let id = cols.next().filter(|s| !s.is_empty())?;
        Some((id.to_string(), cols.next().filter(|s| !s.is_empty()).map(String::from)))
    }).collect()
}

/// `snap list`: a header, then a name and version per line.
pub fn snap_versions(list: &str) -> Vec<(String, String)> {
    list.lines().skip(1).filter_map(|l| {
        let mut cols = l.split_whitespace();
        Some((cols.next()?.to_string(), cols.next()?.to_string()))
    }).collect()
}

pub struct LinuxDetector<M: Machine> {
    pub machine: M,
}

impl<M: Machine> LinuxDetector<M> {
    fn find(&self, p: &Program, dpkg: &[(String, String)], flatpaks: &[(String, Option<String>)], snaps: &[(String, String)]) -> Option<Found> {
        let m = &self.machine;
        let on_path = || m.path_dirs().into_iter().flat_map(|d| p.linux_binaries.iter().map(move |b| format!("{}/{b}", d.trim_end_matches('/')))).find(|f| m.exists(f));
        let installed = || p.linux_paths.iter().find_map(|pat| resolve(m, pat, '/'));
        let package = dpkg.iter().find(|(n, _)| p.linux_packages.contains(&n.as_str())).map(|(_, v)| v.clone());
        let flatpak = p.flatpak.and_then(|id| flatpaks.iter().find(|(n, _)| n == id));
        let snap = p.snap.and_then(|id| snaps.iter().find(|(n, _)| n == id));
        let path = on_path().or_else(installed);
        let version = package.or_else(|| flatpak.and_then(|(_, v)| v.clone())).or_else(|| snap.map(|(_, v)| v.clone()));
        let path = path.or_else(|| flatpak.map(|(id, _)| format!("flatpak:{id}"))).or_else(|| snap.map(|(n, _)| format!("/snap/bin/{n}")));
        (path.is_some() || version.is_some()).then(|| Found { program: p.id, version, path })
    }
}

impl<M: Machine> LocalPrograms for LinuxDetector<M> {
    fn detect(&self) -> Vec<Found> {
        let m = &self.machine;
        let dpkg = m.read(DPKG_STATUS).map(|s| dpkg_versions(&s)).unwrap_or_default();
        let flatpaks = m.listing("flatpak", &["list", "--app", "--columns=application,version"]).map(|s| flatpak_versions(&s)).unwrap_or_default();
        let snaps = m.listing("snap", &["list"]).map(|s| snap_versions(&s)).unwrap_or_default();
        PROGRAMS.iter().filter_map(|p| self.find(p, &dpkg, &flatpaks, &snaps)).collect()
    }
}

/// Whether an uninstall entry's name is the program's: the name alone, or
/// followed by a version ("Blender 4.2", "SOLIDWORKS 2024 SP1"), and not
/// another product that starts the same ("Blender Launcher").
pub fn names_program(display: &str, name: &str) -> bool {
    match display.strip_prefix(name) {
        Some("") => true,
        Some(rest) => rest.strip_prefix(' ').and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_digit()),
        None => false,
    }
}

pub struct WindowsDetector<M: Machine, R: Registry> {
    pub machine: M,
    pub registry: R,
}

impl<M: Machine, R: Registry> WindowsDetector<M, R> {
    fn expand(&self, pattern: &str) -> Option<String> {
        let mut out = pattern.to_string();
        for name in ["LOCALAPPDATA", "PROGRAMFILES"] {
            let key = format!("%{name}%");
            if out.contains(&key) {
                out = out.replace(&key, self.machine.folder(name)?.trim_end_matches('\\'));
            }
        }
        Some(out)
    }
}

impl<M: Machine, R: Registry> LocalPrograms for WindowsDetector<M, R> {
    fn detect(&self) -> Vec<Found> {
        let entries = self.registry.uninstall_entries();
        PROGRAMS
            .iter()
            .filter_map(|p| {
                let entry = entries.iter().find(|e| p.windows_names.iter().any(|n| names_program(&e.name, n)));
                let installed = p.windows_paths.iter().filter_map(|pat| self.expand(pat)).find_map(|pat| resolve(&self.machine, &pat, '\\'));
                let path = installed.or_else(|| entry.and_then(|e| e.location.clone()).map(|l| l.trim_end_matches('\\').to_string()));
                let version = entry.and_then(|e| e.version.clone());
                (entry.is_some() || path.is_some()).then(|| Found { program: p.id, version, path })
            })
            .collect()
    }
}

/// `reg query <Uninstall key> /s`: a key line, then its values.
pub fn parse_reg_query(out: &str) -> Vec<Uninstall> {
    let mut all: Vec<Uninstall> = vec![];
    let mut cur: Option<Uninstall> = None;
    for line in out.lines() {
        if line.starts_with("HKEY_") {
            all.extend(cur.take().filter(|u| !u.name.is_empty()));
            cur = Some(Uninstall::default());
            continue;
        }
        let Some(u) = cur.as_mut() else { continue };
        let mut cols = line.trim_start().splitn(3, "    ");
        let (Some(name), Some(kind), Some(value)) = (cols.next(), cols.next(), cols.next()) else { continue };
        if !kind.starts_with("REG_") {
            continue;
        }
        let value = value.trim().to_string();
        match name {
            "DisplayName" => u.name = value,
            "DisplayVersion" => u.version = Some(value).filter(|v| !v.is_empty()),
            "InstallLocation" => u.location = Some(value).filter(|v| !v.is_empty()),
            _ => {}
        }
    }
    all.extend(cur.filter(|u| !u.name.is_empty()));
    all
}

/// This computer, as it is.
pub struct System;

impl Machine for System {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }
    fn entries(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(path).map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
    }
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
    fn listing(&self, program: &str, args: &[&str]) -> Option<String> {
        let out = quiet(program).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
    // The system's own PATH, read at the system's edge as the shell would.
    fn path_dirs(&self) -> Vec<String> {
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d: PathBuf| d.display().to_string()).collect()).unwrap_or_default()
    }
    fn folder(&self, name: &str) -> Option<String> {
        match name {
            "LOCALAPPDATA" => dirs::data_local_dir().map(|d| d.display().to_string()),
            "PROGRAMFILES" => std::env::var("ProgramFiles").ok(),
            _ => None,
        }
    }
}

/// A command that shows no window of its own.
fn quiet(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// The uninstall keys, machine-wide (both views) and the person's own.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct RegQuery;

#[cfg_attr(not(windows), allow(dead_code))]
const UNINSTALL_KEYS: &[&str] = &[
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

impl Registry for RegQuery {
    fn uninstall_entries(&self) -> Vec<Uninstall> {
        UNINSTALL_KEYS.iter().filter_map(|k| System.listing("reg", &["query", k, "/s"])).flat_map(|out| parse_reg_query(&out)).collect()
    }
}

/// The detector for this system.
pub fn system() -> Box<dyn LocalPrograms> {
    #[cfg(windows)]
    return Box::new(WindowsDetector { machine: System, registry: RegQuery });
    #[cfg(not(windows))]
    return Box::new(LinuxDetector { machine: System });
}

#[cfg(test)]
#[path = "detect_tests.rs"]
mod tests;
