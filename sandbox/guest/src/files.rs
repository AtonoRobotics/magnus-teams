//! Moving files for the kernel. A path under the user's area (`/workspace`)
//! is opened beneath it with openat2(2) and `RESOLVE_BENEATH`: a link that
//! stays inside the area is followed, and one that leads out of it (an
//! absolute link, or `..` past the top) is refused, so `/workspace/link`
//! pointing at `/etc` reaches nothing of the guest. The kernel checks the
//! whole resolution at once, so a link swapped in while the request runs
//! cannot slip past a check made before it. The kernel's own files elsewhere
//! (its CA, a screenshot, an installer) keep plain paths.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

/// The user's area: the volume's mount point.
pub const USER_FILES: &str = "/workspace";
const ERASE_BUFFER_BYTES: usize = 1 << 20;
/// openat2 answers EAGAIN when a rename raced the resolution; it is tried again.
const MAX_RESOLVE_TRIES: u32 = 8;
const NEW_FILE_MODE: u32 = 0o666;
const NEW_DIR_MODE: libc::mode_t = 0o777;

/// Where requests' files are, and the area they may not leave.
pub struct Files {
    root: PathBuf,
}

/// A path's place: beneath the area, as a path relative to its top, or elsewhere.
enum Place {
    Beneath(PathBuf),
    Elsewhere(PathBuf),
}

impl Files {
    pub fn user() -> Files {
        Files { root: PathBuf::from(USER_FILES) }
    }

    #[cfg(test)]
    pub fn at(root: &Path) -> Files {
        Files { root: root.to_path_buf() }
    }

    /// A request's path: relative ones are under the area.
    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn place(&self, path: &Path) -> Place {
        match path.strip_prefix(&self.root) {
            Ok(rel) if rel.as_os_str().is_empty() => Place::Beneath(PathBuf::from(".")),
            Ok(rel) => Place::Beneath(rel.to_path_buf()),
            Err(_) => Place::Elsewhere(path.to_path_buf()),
        }
    }

    fn top(&self) -> io::Result<File> {
        openat2(libc::AT_FDCWD, &self.root, libc::O_PATH | libc::O_DIRECTORY, 0, 0)
    }

    fn open(&self, path: &Path, flags: libc::c_int, mode: u32) -> io::Result<File> {
        match self.place(path) {
            Place::Beneath(rel) => openat2(self.top()?.as_raw_fd(), &rel, flags, mode, libc::RESOLVE_BENEATH | libc::RESOLVE_NO_MAGICLINKS),
            Place::Elsewhere(p) => openat2(libc::AT_FDCWD, &p, flags, mode, 0),
        }
    }

    /// The directory holding `path`, opened as above, and the name in it.
    fn parent_of(&self, path: &Path) -> io::Result<(File, CString)> {
        let name = path.file_name().ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("/"));
        Ok((self.open(parent, libc::O_PATH | libc::O_DIRECTORY, 0)?, CString::new(name.as_bytes())?))
    }

    /// Every missing directory above `path`, each made in a parent opened as above.
    fn create_parents(&self, path: &Path) -> io::Result<()> {
        let Some(parent) = path.parent() else { return Ok(()) };
        let mut at = PathBuf::new();
        for part in parent.components() {
            at.push(part);
            if !matches!(part, Component::Normal(_)) {
                continue;
            }
            match self.open(&at, libc::O_PATH | libc::O_DIRECTORY, 0) {
                Ok(_) => continue,
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
                Err(e) => return Err(e),
            }
            let (dir, name) = self.parent_of(&at)?;
            if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), NEW_DIR_MODE) } != 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() != Some(libc::EEXIST) {
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// The whole file, or the part of it asked for.
    pub fn read(&self, path: &Path, offset: Option<u64>, length: Option<u64>) -> io::Result<Vec<u8>> {
        let mut file = self.open(path, libc::O_RDONLY, 0)?;
        let mut out = Vec::new();
        match length {
            None => {
                file.read_to_end(&mut out)?;
            }
            Some(length) => {
                file.seek(io::SeekFrom::Start(offset.unwrap_or(0)))?;
                file.take(length).read_to_end(&mut out)?;
            }
        }
        Ok(out)
    }

    pub fn write(&self, path: &Path, data: &[u8], append: bool) -> io::Result<()> {
        let _ = self.create_parents(path);
        let how = if append { libc::O_APPEND } else { libc::O_TRUNC };
        self.open(path, libc::O_WRONLY | libc::O_CREAT | how, NEW_FILE_MODE)?.write_all(data)
    }

    /// A directory's entries; each entry's metadata is its own, links not followed.
    pub fn read_dir(&self, path: &Path) -> io::Result<std::fs::ReadDir> {
        let dir = self.open(path, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))
    }

    /// Zeroes a file before unlinking it, since a freed block would otherwise
    /// stay readable in the drive image and in every later backup of it. The
    /// file itself is never a link.
    pub fn erase(&self, path: &Path) -> io::Result<()> {
        let mut file = self.open(path, libc::O_WRONLY | libc::O_NOFOLLOW, 0)?;
        let mut left = file.metadata()?.len();
        let zeros = vec![0u8; ERASE_BUFFER_BYTES];
        while left > 0 {
            let n = left.min(ERASE_BUFFER_BYTES as u64) as usize;
            file.write_all(&zeros[..n])?;
            left -= n as u64;
        }
        file.sync_all()?;
        let (dir, name) = self.parent_of(path)?;
        if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { libc::sync() };
        Ok(())
    }
}

fn openat2(dir: RawFd, path: &Path, flags: libc::c_int, mode: u32, resolve: u64) -> io::Result<File> {
    let c = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: open_how is plain integers; all-zero is its documented default.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (flags | libc::O_CLOEXEC) as u64;
    how.mode = u64::from(mode);
    how.resolve = resolve;
    for _ in 0..MAX_RESOLVE_TRIES {
        let fd = unsafe { libc::syscall(libc::SYS_openat2, dir, c.as_ptr(), &how as *const libc::open_how, std::mem::size_of::<libc::open_how>()) };
        if fd >= 0 {
            return Ok(unsafe { File::from_raw_fd(fd as RawFd) });
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EAGAIN) => continue,
            Some(libc::EXDEV) => return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("leads out of {USER_FILES}"))),
            _ => return Err(e),
        }
    }
    Err(io::Error::from_raw_os_error(libc::EAGAIN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A user area with a file inside, and beside it a directory outside the
    /// area that a link inside points at.
    struct Fixture {
        top: PathBuf,
        area: PathBuf,
        outside: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let top = std::env::temp_dir().join(format!("magnus-guest-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&top);
            let (area, outside) = (top.join("workspace"), top.join("etc"));
            std::fs::create_dir_all(area.join("docs")).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::write(outside.join("passwd"), b"root:secret").unwrap();
            std::fs::write(area.join("docs/note.txt"), b"mine").unwrap();
            symlink(&outside, area.join("link")).unwrap();
            symlink("docs", area.join("inside")).unwrap();
            Fixture { top, area, outside }
        }

        fn files(&self) -> Files {
            Files::at(&self.area)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    #[test]
    fn a_read_through_a_link_out_of_the_area_is_refused() {
        let f = Fixture::new("read");
        let escaping = f.area.join("link/passwd");
        // A plain open follows the link out: what the guest did before.
        assert_eq!(std::fs::read(&escaping).unwrap(), b"root:secret");
        let err = f.files().read(&escaping, None, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
        assert!(f.files().read(&escaping, Some(0), Some(4)).is_err());
        assert!(f.files().read(&f.area.join("../etc/passwd"), None, None).is_err());
        assert_eq!(f.files().read(&f.area.join("docs/note.txt"), None, None).unwrap(), b"mine");
        // A link that stays inside still works.
        assert_eq!(f.files().read(&f.area.join("inside/note.txt"), Some(1), Some(2)).unwrap(), b"in");
    }

    #[test]
    fn an_absolute_link_is_refused_even_to_somewhere_inside() {
        let f = Fixture::new("absolute");
        symlink(f.area.join("docs"), f.area.join("abs")).unwrap();
        assert!(f.files().read(&f.area.join("abs/note.txt"), None, None).is_err());
    }

    #[test]
    fn a_write_through_a_link_out_of_the_area_changes_nothing_outside() {
        let f = Fixture::new("write");
        assert!(f.files().write(&f.area.join("link/passwd"), b"owned", false).is_err());
        assert!(f.files().write(&f.area.join("link/passwd"), b"owned", true).is_err());
        assert!(f.files().write(&f.area.join("link/new/deeper.txt"), b"x", false).is_err());
        assert_eq!(std::fs::read(f.outside.join("passwd")).unwrap(), b"root:secret");
        assert!(!f.outside.join("new").exists(), "a directory was made outside the area");
        // A file whose last part is a link out is refused too.
        symlink(f.outside.join("passwd"), f.area.join("pw")).unwrap();
        assert!(f.files().write(&f.area.join("pw"), b"owned", false).is_err());
        assert_eq!(std::fs::read(f.outside.join("passwd")).unwrap(), b"root:secret");
        // Writing inside makes the folders it needs, and appends.
        f.files().write(&f.area.join("a/b/c.txt"), b"one", false).unwrap();
        f.files().write(&f.area.join("a/b/c.txt"), b"two", true).unwrap();
        assert_eq!(std::fs::read(f.area.join("a/b/c.txt")).unwrap(), b"onetwo");
    }

    #[test]
    fn a_listing_through_a_link_out_of_the_area_is_refused() {
        let f = Fixture::new("list");
        assert!(f.files().read_dir(&f.area.join("link")).is_err());
        let names: Vec<String> = f.files().read_dir(&f.area).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"link".to_string()) && names.contains(&"docs".to_string()), "{names:?}");
        // The link is listed as itself, not as the directory it points at.
        let link = f.files().read_dir(&f.area).unwrap().flatten().find(|e| e.file_name() == "link").unwrap();
        assert!(link.metadata().unwrap().file_type().is_symlink());
    }

    #[test]
    fn an_erase_through_a_link_out_of_the_area_leaves_the_file() {
        let f = Fixture::new("erase");
        assert!(f.files().erase(&f.area.join("link/passwd")).is_err());
        assert_eq!(std::fs::read(f.outside.join("passwd")).unwrap(), b"root:secret");
        symlink(f.outside.join("passwd"), f.area.join("pw")).unwrap();
        assert!(f.files().erase(&f.area.join("pw")).is_err());
        assert_eq!(std::fs::read(f.outside.join("passwd")).unwrap(), b"root:secret");
        f.files().erase(&f.area.join("docs/note.txt")).unwrap();
        assert!(!f.area.join("docs/note.txt").exists());
    }

    #[test]
    fn only_paths_under_the_area_are_confined() {
        let files = Files::user();
        assert!(matches!(files.place(Path::new("/workspace/a")), Place::Beneath(_)));
        assert!(matches!(files.place(Path::new("/workspace")), Place::Beneath(_)));
        assert!(matches!(files.place(Path::new("/workspace/../etc/passwd")), Place::Beneath(_)));
        assert!(matches!(files.place(Path::new("/workspaces/a")), Place::Elsewhere(_)));
        assert!(matches!(files.place(Path::new("/tmp/magnus-shot.png")), Place::Elsewhere(_)));
        assert_eq!(files.resolve("a/b"), Path::new("/workspace/a/b"));
    }
}
