//! A brokered connector's server image (connectors spec, Brokered API
//! connectors: "one server image per provider, identified in its manifest by
//! digest"), built by `brokered/build.sh`.

use magnus_scratch::brokered::{image, image_elsewhere};
use sha2::{Digest, Sha256};

/// Built from the same commit in two places, the image has one digest, the
/// SHA-256 of its file; it holds the server alone as `/init`, a static
/// program owned by root with no time, and nothing else.
#[test]
fn a_server_image_is_built_reproducibly_and_named_by_its_digest() {
    let a = image("magnus-brokered-test");
    let target = tempfile::tempdir().unwrap();
    let b = image_elsewhere("magnus-brokered-test", target.path());
    assert_eq!(a.digest, b.digest, "two builds of one commit differ");
    let bytes = std::fs::read(a.file()).unwrap();
    assert_eq!(a.digest, format!("sha256:{}", Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>()));

    // newc: each entry is a 110-byte header ("070701", then 13 eight-digit
    // hex fields), its name padded to four bytes, its data padded to four.
    let mut entries = vec![];
    let mut at = 0;
    loop {
        let h = &bytes[at..at + 110];
        assert_eq!(&h[..6], b"070701");
        let field = |i: usize| u32::from_str_radix(std::str::from_utf8(&h[6 + 8 * i..14 + 8 * i]).unwrap(), 16).unwrap();
        let (mode, uid, gid, mtime, size, namesize) = (field(1), field(2), field(3), field(5), field(6) as usize, field(11) as usize);
        let name = std::str::from_utf8(&bytes[at + 110..at + 110 + namesize - 1]).unwrap().to_string();
        let data = (at + 110 + namesize + 3) & !3;
        if name == "TRAILER!!!" {
            break;
        }
        entries.push((name, mode, uid, gid, mtime, bytes[data..data + size].to_vec()));
        at = (data + size + 3) & !3;
    }
    assert_eq!(entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["init"]);
    let (_, mode, uid, gid, mtime, init) = &entries[0];
    assert_eq!((mode & 0o170777, *uid, *gid, *mtime), (0o100755, 0, 0, 0));
    assert_eq!(&init[..4], b"\x7fELF");
    // Static: no program header asks for an interpreter (PT_INTERP, 3).
    let (phoff, phentsize, phnum) = (
        u64::from_le_bytes(init[0x20..0x28].try_into().unwrap()) as usize,
        u16::from_le_bytes(init[0x36..0x38].try_into().unwrap()) as usize,
        u16::from_le_bytes(init[0x38..0x3a].try_into().unwrap()) as usize,
    );
    assert!((0..phnum).all(|i| u32::from_le_bytes(init[phoff + i * phentsize..phoff + i * phentsize + 4].try_into().unwrap()) != 3), "/init is not static");
}

/// First release run on the magnus-teams server (2026-11-09): every
/// brokered manifest in the catalog still named the image its first commit
/// built; the locked dependencies had moved since, so no checkout built any
/// of them and release/to-appliance.sh refused the release. Each image a
/// manifest names is one this checkout builds.
#[test]
fn every_server_image_the_catalog_names_is_built_from_this_checkout() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let packages = std::fs::read_to_string(root.join("release/brokered-images")).unwrap();
    let built: Vec<String> = packages.lines().map(str::trim).filter(|p| !p.is_empty()).map(|p| image(p).digest).collect();
    for entry in std::fs::read_dir(root.join("catalog")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let m: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if m.get("kind").and_then(|k| k.as_str()) == Some("brokered") {
            let named = m["endpoint"].as_str().unwrap();
            assert!(built.iter().any(|d| d == named), "{} names {named}, which this checkout does not build; it builds {built:?}", path.display());
        }
    }
}
