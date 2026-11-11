use super::*;
use std::cell::RefCell;

const PROGRAM_BYTES: &[u8] = b"#!/bin/sh\necho new computerd\n";
const KERNEL_BYTES: &[u8] = b"a kernel";
const ROOTFS_BYTES: &[u8] = b"a base image";

fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

struct Release {
    _dir: tempfile::TempDir,
    paths: Paths,
    secret: minisign::SecretKey,
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn signed(secret: &minisign::SecretKey, data: &[u8], version: &str) -> String {
    let sig = minisign::sign(None, secret, data, Some(&format!("timestamp:0\tfile:package\tversion:{version}")), None).unwrap();
    b64(&sig.into_string())
}

/// A package as `release/publish.sh` makes one: `./`-prefixed members, the
/// images as links into the user's sandbox directory.
fn package(version: &str, hashes: &str) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let mut add = |name: &str, data: &[u8]| {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(&mut h, format!("./{name}"), data).unwrap();
    };
    add("VERSION", format!("{version}\n").as_bytes());
    add("magnus-computerd", PROGRAM_BYTES);
    add("install-computerd.sh", b"#!/bin/sh\n");
    add("magnus-computerd.service", b"[Service]\n");
    add("images.sha256", hashes.as_bytes());
    add("magnus", b"the kernel program, not root's business");
    for name in ["vmlinux", "rootfs.ext4"] {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_cksum();
        b.append_link(&mut h, format!("./{name}"), format!("../sandbox/abc/{name}")).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

fn good_hashes() -> String {
    format!("{}  vmlinux\n{}  rootfs.ext4\n", sha(KERNEL_BYTES), sha(ROOTFS_BYTES))
}

/// A user's release directory with `version` published and signed, and a
/// root side with `installed` installed.
fn release(version: &str, installed: &str) -> Release {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let paths = Paths {
        releases: root.join("updates/appliance"),
        links_from: root.join("bin"),
        key: root.join("etc/release.pub"),
        program_dir: root.join("lib"),
        images_dir: root.join("state/images"),
        staging: root.join("state/update"),
        owner: None,
    };
    for d in [&paths.releases, &paths.links_from, &paths.program_dir, &paths.images_dir, &root.join("etc"), &root.join("sandbox/abc")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(&paths.key, b64(&kp.pk.to_box().unwrap().into_string())).unwrap();
    std::fs::write(paths.program_dir.join("VERSION"), format!("{installed}\n")).unwrap();
    std::fs::write(root.join("sandbox/abc/vmlinux"), KERNEL_BYTES).unwrap();
    std::fs::write(root.join("sandbox/abc/rootfs.ext4"), ROOTFS_BYTES).unwrap();
    let r = Release { _dir: dir, paths, secret: kp.sk };
    r.publish(version, &package(version, &good_hashes()), None);
    r
}

impl Release {
    fn publish(&self, version: &str, data: &[u8], signature: Option<String>) {
        let file = format!("magnus-appliance-{version}.tar.gz");
        std::fs::write(self.paths.releases.join(&file), data).unwrap();
        let signature = signature.unwrap_or_else(|| signed(&self.secret, data, version));
        let m = serde_json::json!({ "version": version, "file": file, "signature": signature });
        std::fs::write(self.paths.releases.join("latest.json"), m.to_string()).unwrap();
    }

    fn file(&self, version: &str) -> PathBuf {
        self.paths.releases.join(format!("magnus-appliance-{version}.tar.gz"))
    }

    /// Run the update; what the installer was handed, if it ran.
    fn update(&self) -> (Result<Outcome>, Option<Staged>) {
        let got = RefCell::new(None);
        let r = run(&self.paths, |s| {
            assert_eq!(std::fs::read(&s.program).unwrap(), PROGRAM_BYTES);
            assert_eq!(std::fs::read(&s.kernel).unwrap(), KERNEL_BYTES);
            assert_eq!(std::fs::read(&s.rootfs).unwrap(), ROOTFS_BYTES);
            assert!(s.program.starts_with(&self.paths.staging) && s.installer.starts_with(&self.paths.staging));
            *got.borrow_mut() =
                Some(Staged { version: s.version.clone(), installer: s.installer.clone(), program: s.program.clone(), kernel: s.kernel.clone(), rootfs: s.rootfs.clone() });
            Ok(())
        });
        (r, got.into_inner())
    }
}

#[test]
fn a_newer_signed_release_is_installed_from_roots_own_copy() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    let (out, staged) = r.update();
    assert_eq!(out.unwrap(), Outcome::Installed { from: "0.2.6-test.80".into(), to: "0.2.6-test.81".into() });
    let s = staged.expect("the installer ran");
    assert!(s.kernel.starts_with(&r.paths.staging), "the image was copied into root's directory, not used where the user can change it");
    assert!(!r.paths.staging.exists(), "the staging directory is cleared after");
}

#[test]
fn an_unsigned_release_is_refused() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    r.publish("0.2.6-test.81", &package("0.2.6-test.81", &good_hashes()), Some(String::new()));
    let (out, staged) = r.update();
    assert!(out.is_err());
    assert!(staged.is_none(), "nothing unsigned reaches the installer");
}

#[test]
fn a_tampered_package_is_refused() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    let mut bytes = std::fs::read(r.file("0.2.6-test.81")).unwrap();
    let last = bytes.len() - 20;
    bytes[last] ^= 1;
    std::fs::write(r.file("0.2.6-test.81"), bytes).unwrap();
    let (out, staged) = r.update();
    assert!(format!("{:#}", out.unwrap_err()).contains("does not verify"));
    assert!(staged.is_none());
}

#[test]
fn a_package_signed_by_another_key_is_refused() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let data = package("0.2.6-test.81", &good_hashes());
    r.publish("0.2.6-test.81", &data, Some(signed(&other.sk, &data, "0.2.6-test.81")));
    let (out, staged) = r.update();
    assert!(out.is_err());
    assert!(staged.is_none());
}

#[test]
fn a_package_signed_as_another_version_is_refused() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    let data = package("0.2.6-test.99", &good_hashes());
    r.publish("0.2.6-test.99", &data, Some(signed(&r.secret, &data, "0.2.6-test.81")));
    let (out, staged) = r.update();
    assert!(format!("{:#}", out.unwrap_err()).contains("signed as"));
    assert!(staged.is_none());
}

#[test]
fn an_image_changed_after_signing_is_refused() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    std::fs::write(r.paths.links_from.join("../sandbox/abc/rootfs.ext4"), b"a base image with a back door").unwrap();
    let (out, staged) = r.update();
    assert!(format!("{:#}", out.unwrap_err()).contains("is not the rootfs.ext4 the release was signed with"));
    assert!(staged.is_none());
    assert!(!r.paths.staging.exists(), "the copy is not left behind");
}

#[test]
fn an_older_or_the_same_release_is_not_installed() {
    for installed in ["0.2.6-test.81", "0.2.6-test.90", "0.2.6"] {
        let r = release("0.2.6-test.81", installed);
        let (out, staged) = r.update();
        assert!(matches!(out.unwrap(), Outcome::UpToDate { .. }), "{installed}");
        assert!(staged.is_none(), "{installed}");
    }
}

#[test]
fn an_unchanged_image_is_taken_from_roots_own_copy() {
    let r = release("0.2.6-test.81", "0.2.6-test.80");
    std::fs::write(r.paths.images_dir.join("vmlinux"), KERNEL_BYTES).unwrap();
    std::fs::write(r.paths.images_dir.join("rootfs.ext4"), ROOTFS_BYTES).unwrap();
    std::fs::write(r.paths.images_dir.join("base.id"), format!("{}\n", sha(ROOTFS_BYTES))).unwrap();
    std::fs::remove_file(r.paths.links_from.join("../sandbox/abc/rootfs.ext4")).unwrap();
    let (out, staged) = r.update();
    assert!(matches!(out.unwrap(), Outcome::Installed { .. }));
    let s = staged.unwrap();
    assert_eq!(s.rootfs, r.paths.images_dir.join("rootfs.ext4"));
    assert_eq!(s.kernel, r.paths.images_dir.join("vmlinux"));
}

#[test]
fn the_built_in_key_is_the_release_key() {
    let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(BUILT_IN_KEY.trim()).unwrap()).unwrap();
    assert!(minisign_verify::PublicKey::decode(&text).is_ok());
}
