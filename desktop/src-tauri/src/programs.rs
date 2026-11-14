//! The programs the bridge supports, and where each is found on Linux and
//! Windows (docs/connectors.md, Programs). Detection data only: each
//! program's MCP server, and so its loopback port, comes in its own phase.

// Each system's detector is built and tested everywhere; only its own runs.
#![cfg_attr(windows, allow(dead_code))]

/// How one program is found.
pub struct Program {
    /// The id the server knows it by (`crates/magnus/src/desktop.rs`).
    pub id: &'static str,
    /// Executable names looked up on the PATH.
    pub linux_binaries: &'static [&'static str],
    /// Debian package names, read from the package records.
    pub linux_packages: &'static [&'static str],
    pub flatpak: Option<&'static str>,
    pub snap: Option<&'static str>,
    /// Install paths; a component ending in `*` matches any entry that starts with the rest.
    pub linux_paths: &'static [&'static str],
    /// An uninstall entry whose DisplayName is one of these, alone or followed by a version.
    pub windows_names: &'static [&'static str],
    /// As `linux_paths`; `%LOCALAPPDATA%` and `%PROGRAMFILES%` stand for those folders.
    pub windows_paths: &'static [&'static str],
    /// The loopback port its MCP server answers on, once its phase sets it.
    pub mcp_port: Option<u16>,
}

pub const PROGRAMS: &[Program] = &[
    Program {
        id: "blender",
        linux_binaries: &["blender"],
        linux_packages: &["blender"],
        flatpak: Some("org.blender.Blender"),
        snap: Some("blender"),
        linux_paths: &["/opt/blender/blender", "/opt/blender*/blender", "/usr/local/blender/blender"],
        windows_names: &["Blender"],
        windows_paths: &["%PROGRAMFILES%\\Blender Foundation\\Blender*\\blender.exe"],
        mcp_port: None,
    },
    Program {
        id: "freecad",
        linux_binaries: &["freecad", "FreeCAD"],
        linux_packages: &["freecad"],
        flatpak: Some("org.freecad.FreeCAD"),
        snap: Some("freecad"),
        linux_paths: &["/opt/freecad/bin/FreeCAD", "/opt/FreeCAD*/bin/FreeCAD"],
        windows_names: &["FreeCAD"],
        windows_paths: &["%PROGRAMFILES%\\FreeCAD*\\bin\\FreeCAD.exe"],
        mcp_port: None,
    },
    Program {
        id: "fusion",
        linux_binaries: &[],
        linux_packages: &[],
        flatpak: None,
        snap: None,
        linux_paths: &[],
        windows_names: &["Autodesk Fusion"],
        windows_paths: &["%LOCALAPPDATA%\\Autodesk\\webdeploy\\production\\*\\Fusion360.exe"],
        mcp_port: None,
    },
    Program {
        id: "solidworks",
        linux_binaries: &[],
        linux_packages: &[],
        flatpak: None,
        snap: None,
        linux_paths: &[],
        windows_names: &["SOLIDWORKS"],
        windows_paths: &["%PROGRAMFILES%\\SOLIDWORKS Corp\\SOLIDWORKS\\SLDWORKS.exe"],
        mcp_port: None,
    },
];

pub fn program(id: &str) -> Option<&'static Program> {
    PROGRAMS.iter().find(|p| p.id == id)
}
