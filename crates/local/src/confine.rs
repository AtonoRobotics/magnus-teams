//! What a command on the person's machine runs inside. Linux has bubblewrap;
//! macOS has `sandbox-exec`, which refuses all but what is listed here.
use crate::Folders;
use std::path::Path;

const SYSTEM_ROOTS: &[&str] = &["/bin", "/sbin", "/lib", "/lib64"];
/// What a command needs to find a host and trust it, and nothing else of /etc.
const NET_FILES: &[&str] = &["/etc/resolv.conf", "/etc/hosts", "/etc/ssl", "/etc/ca-certificates", "/etc/nsswitch.conf"];
/// System paths a program needs to start on macOS. The keychains and every
/// home directory are absent.
const MAC_READ: &[&str] = &["/usr", "/bin", "/sbin", "/System", "/private/var/db/dyld", "/opt/homebrew/bin", "/opt/homebrew/lib", "/opt/homebrew/Cellar", "/Library/Apple"];
/// The system services a program needs. The security server, which fronts the
/// keychain, is not one of them.
const MAC_SERVICES: &[&str] = &["com.apple.system.logger", "com.apple.system.notification_center", "com.apple.system.opendirectoryd.libinfo"];

pub(crate) fn bubblewrap_args(cwd: &Path, folders: &Folders, command: &str, network: bool) -> Vec<String> {
    let mut a: Vec<String> =
        ["--unshare-all", "--die-with-parent", "--new-session", "--ro-bind", "/usr", "/usr", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/run"]
            .map(String::from)
            .to_vec();
    for root in SYSTEM_ROOTS {
        match std::fs::read_link(root) {
            Ok(target) => a.extend(["--symlink".to_string(), target.display().to_string(), root.to_string()]),
            Err(_) if Path::new(root).exists() => a.extend(["--ro-bind".to_string(), root.to_string(), root.to_string()]),
            Err(_) => {}
        }
    }
    if network {
        a.push("--share-net".to_string());
        for f in NET_FILES {
            a.extend(["--ro-bind-try".to_string(), f.to_string(), f.to_string()]);
        }
    }
    for f in folders.list() {
        a.extend(["--bind".to_string(), f.clone(), f]);
    }
    a.extend(["--chdir".to_string(), cwd.display().to_string(), "--".to_string(), "/bin/sh".to_string(), "-c".to_string(), command.to_string()]);
    a
}

fn quoted(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A sandbox-exec profile: nothing but the system, the granted folders and
/// the network if this command was approved to use it.
pub(crate) fn seatbelt_profile(folders: &Folders, network: bool) -> String {
    let mut p = String::from("(version 1)\n(deny default)\n(allow process-fork)\n(allow process-exec)\n(allow signal (target same-sandbox))\n(allow sysctl-read)\n");
    for s in MAC_SERVICES {
        p.push_str(&format!("(allow mach-lookup (global-name {}))\n", quoted(s)));
    }
    for r in MAC_READ {
        p.push_str(&format!("(allow file-read* (subpath {}))\n", quoted(r)));
    }
    p.push_str("(allow file-read-metadata)\n(allow file* (literal \"/dev/null\") (literal \"/dev/urandom\") (literal \"/dev/random\"))\n");
    for f in folders.list() {
        p.push_str(&format!("(allow file* (subpath {}))\n", quoted(&f)));
    }
    if network {
        p.push_str("(allow network*)\n(allow mach-lookup (global-name \"com.apple.dnssd.service\"))\n");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders() -> (tempfile::TempDir, Folders) {
        let d = tempfile::tempdir().unwrap();
        let f = Folders::new(&[d.path().to_path_buf()]).unwrap();
        (d, f)
    }

    #[test]
    fn a_macos_command_gets_no_network_keychain_or_home_unless_listed() {
        let (d, f) = folders();
        let p = seatbelt_profile(&f, false);
        assert!(p.starts_with("(version 1)\n(deny default)"), "{p}");
        assert!(p.contains(&d.path().canonicalize().unwrap().display().to_string()));
        assert!(!p.contains("network"), "{p}");
        assert!(!p.contains("Keychains") && !p.contains("SecurityServer") && !p.contains("/Users"), "{p}");
        assert!(seatbelt_profile(&f, true).contains("(allow network*)"));
    }

    #[test]
    fn a_folder_name_cannot_write_its_own_rules() {
        assert_eq!(quoted("/a\"b\\c"), "\"/a\\\"b\\\\c\"");
    }

    #[test]
    fn only_an_approved_command_shares_the_network() {
        let (d, f) = folders();
        assert!(!bubblewrap_args(d.path(), &f, "true", false).iter().any(|a| a == "--share-net"));
        let with = bubblewrap_args(d.path(), &f, "true", true);
        assert!(with.iter().any(|a| a == "--share-net"));
        assert!(with.contains(&"/etc/resolv.conf".to_string()) && !with.contains(&"/etc/passwd".to_string()));
    }
}
