use super::*;
use std::collections::BTreeMap;

/// A computer made of a few files, folders, listings and a PATH.
#[derive(Default)]
struct Fake {
    files: Vec<String>,
    reads: BTreeMap<String, String>,
    listings: BTreeMap<String, String>,
    path: Vec<String>,
    folders: BTreeMap<String, String>,
}

impl Fake {
    fn with(files: &[&str]) -> Fake {
        Fake { files: files.iter().map(|s| s.to_string()).collect(), ..Fake::default() }
    }
}

impl Machine for Fake {
    fn exists(&self, path: &str) -> bool {
        self.files.iter().any(|f| f == path)
    }
    fn entries(&self, path: &str) -> Vec<String> {
        let sep = if path.contains('\\') { '\\' } else { '/' };
        let prefix = format!("{path}{sep}");
        let mut names: Vec<String> = self.files.iter().filter_map(|f| f.strip_prefix(&prefix)).filter_map(|rest| rest.split(sep).next()).map(String::from).collect();
        names.dedup();
        names
    }
    fn read(&self, path: &str) -> Option<String> {
        self.reads.get(path).cloned()
    }
    fn listing(&self, program: &str, _args: &[&str]) -> Option<String> {
        self.listings.get(program).cloned()
    }
    fn path_dirs(&self) -> Vec<String> {
        self.path.clone()
    }
    fn folder(&self, name: &str) -> Option<String> {
        self.folders.get(name).cloned()
    }
}

struct FakeRegistry(Vec<Uninstall>);

impl Registry for FakeRegistry {
    fn uninstall_entries(&self) -> Vec<Uninstall> {
        self.0.clone()
    }
}

fn found(list: &[Found], id: &str) -> Option<Found> {
    list.iter().find(|f| f.program == id).cloned()
}

#[test]
fn nothing_installed_is_nothing_found() {
    assert!(LinuxDetector { machine: Fake::default() }.detect().is_empty());
    assert!(WindowsDetector { machine: Fake::default(), registry: FakeRegistry(vec![]) }.detect().is_empty());
}

/// On the PATH, with its version from the package records.
#[test]
fn linux_finds_a_program_on_the_path_with_its_package_version() {
    let mut m = Fake::with(&["/usr/bin/blender"]);
    m.path = vec!["/usr/local/bin".into(), "/usr/bin/".into()];
    m.reads.insert(
        DPKG_STATUS.into(),
        "Package: freecad\nStatus: deinstall ok config-files\nVersion: 0.20\n\nPackage: blender\nStatus: install ok installed\nVersion: 4.0.2+dfsg-1\n".into(),
    );
    let all = LinuxDetector { machine: m }.detect();
    assert_eq!(found(&all, "blender"), Some(Found { program: "blender", version: Some("4.0.2+dfsg-1".into()), path: Some("/usr/bin/blender".into()) }));
    assert_eq!(found(&all, "freecad"), None, "a removed package counted as installed");
}

#[test]
fn linux_finds_flatpak_and_snap_installs() {
    let mut m = Fake::default();
    m.listings.insert("flatpak".into(), "org.blender.Blender\t4.2.1\norg.gimp.GIMP\t2.10\n".into());
    m.listings.insert("snap".into(), "Name     Version  Rev  Tracking\nfreecad  1.0.0    123  latest/stable\n".into());
    let all = LinuxDetector { machine: m }.detect();
    assert_eq!(found(&all, "blender"), Some(Found { program: "blender", version: Some("4.2.1".into()), path: Some("flatpak:org.blender.Blender".into()) }));
    assert_eq!(found(&all, "freecad"), Some(Found { program: "freecad", version: Some("1.0.0".into()), path: Some("/snap/bin/freecad".into()) }));
    assert_eq!(all.len(), 2, "a Windows-only program was found on Linux: {all:?}");
}

/// An install path with a version in its folder name: the newest one.
#[test]
fn linux_finds_an_install_path_by_its_pattern() {
    let m = Fake::with(&["/opt/blender-4.1/blender", "/opt/blender-4.2/blender", "/opt/blender-4.3/README"]);
    let all = LinuxDetector { machine: m }.detect();
    assert_eq!(found(&all, "blender").and_then(|f| f.path).as_deref(), Some("/opt/blender-4.2/blender"), "a folder without the program was taken");
}

#[test]
fn a_pattern_takes_the_newest_folder_that_has_the_file() {
    let m = Fake::with(&["/opt/blender-4.1/blender", "/opt/blender-4.2/blender"]);
    assert_eq!(resolve(&m, "/opt/blender*/blender", '/').as_deref(), Some("/opt/blender-4.2/blender"));
    assert_eq!(resolve(&m, "/opt/freecad*/bin/FreeCAD", '/'), None);
}

/// The uninstall keys name the program and its version; another product
/// whose name starts the same is not it.
#[test]
fn windows_finds_programs_by_their_uninstall_entries() {
    let reg = FakeRegistry(vec![
        Uninstall { name: "Blender Launcher".into(), version: Some("1.0".into()), location: None },
        Uninstall { name: "Blender 4.2".into(), version: Some("4.2.3".into()), location: Some("C:\\Program Files\\Blender Foundation\\Blender 4.2\\".into()) },
        Uninstall { name: "SOLIDWORKS 2024 SP1".into(), version: Some("32.1.0".into()), location: None },
        Uninstall { name: "SOLIDWORKS eDrawings 2024".into(), version: Some("32.0".into()), location: None },
    ]);
    let all = WindowsDetector { machine: Fake::default(), registry: reg }.detect();
    assert_eq!(found(&all, "blender"), Some(Found { program: "blender", version: Some("4.2.3".into()), path: Some("C:\\Program Files\\Blender Foundation\\Blender 4.2".into()) }));
    assert_eq!(found(&all, "solidworks").and_then(|f| f.version), Some("32.1.0".into()));
    assert_eq!(all.len(), 2, "{all:?}");
}

/// Fusion installs under the person's own folder, with no uninstall entry.
#[test]
fn windows_finds_an_install_path_under_the_persons_folder() {
    let mut m = Fake::with(&["C:\\Users\\mo\\AppData\\Local\\Autodesk\\webdeploy\\production\\abc123\\Fusion360.exe"]);
    m.folders.insert("LOCALAPPDATA".into(), "C:\\Users\\mo\\AppData\\Local".into());
    let all = WindowsDetector { machine: m, registry: FakeRegistry(vec![]) }.detect();
    assert_eq!(
        found(&all, "fusion"),
        Some(Found { program: "fusion", version: None, path: Some("C:\\Users\\mo\\AppData\\Local\\Autodesk\\webdeploy\\production\\abc123\\Fusion360.exe".into()) })
    );
}

#[test]
fn a_reg_query_is_read_entry_by_entry() {
    let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{A}\r\n    DisplayName    REG_SZ    FreeCAD 1.0.0\r\n    DisplayVersion    REG_SZ    1.0.0\r\n    InstallLocation    REG_SZ    C:\\Program Files\\FreeCAD 1.0\r\n\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{B}\r\n    SystemComponent    REG_DWORD    0x1\r\n";
    let all = parse_reg_query(out);
    assert_eq!(all, vec![Uninstall { name: "FreeCAD 1.0.0".into(), version: Some("1.0.0".into()), location: Some("C:\\Program Files\\FreeCAD 1.0".into()) }]);
    assert!(names_program("FreeCAD 1.0.0", "FreeCAD") && names_program("Autodesk Fusion", "Autodesk Fusion") && !names_program("FreeCADx", "FreeCAD"));
}

#[test]
fn package_listings_are_read() {
    assert_eq!(dpkg_versions("Package: a\nStatus: install ok installed\nVersion: 1\n\nPackage: b\nVersion: 2\n"), vec![("a".to_string(), "1".to_string())]);
    assert_eq!(flatpak_versions("org.x.Y\n\n"), vec![("org.x.Y".to_string(), None)]);
    assert!(snap_versions("Name Version\n").is_empty());
}
