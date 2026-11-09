//! Zeroing a volume's free space writes every block of a sparse file, so the
//! host needs room for it first and gets the blocks back afterwards.
use super::*;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};

const HOLE_BLOCK_BYTES: usize = 64 * 1024;
/// Left free on the host after a volume is filled.
const HOST_RESERVE_BYTES: u64 = 4 * BYTES_PER_GIB;
/// The unit of `st_blocks`.
const STAT_BLOCK_BYTES: u64 = 512;

fn enough_room(available: u64, volume: u64, allocated: u64) -> bool {
    available >= volume.saturating_sub(allocated).saturating_add(HOST_RESERVE_BYTES)
}

fn available_bytes(path: &Path) -> Result<u64> {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).context("a path with a nul in it")?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(name.as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error()).context("read the host's free space");
    }
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// Turns every all-zero block into a hole. Contents read the same.
fn dig(file: &File) -> Result<u64> {
    let size = file.metadata().context("read the volume's size")?.len();
    let fd = file.as_raw_fd();
    let block_bytes = HOLE_BLOCK_BYTES as u64;
    let mut buffer = vec![0u8; HOLE_BLOCK_BYTES];
    let (mut at, mut dug) = (0u64, 0u64);
    while at < size {
        let data = unsafe { libc::lseek(fd, at as libc::off_t, libc::SEEK_DATA) };
        if data < 0 {
            let e = std::io::Error::last_os_error();
            // Nothing but hole from here on.
            if e.raw_os_error() == Some(libc::ENXIO) {
                break;
            }
            return Err(e).context("find the volume's next data");
        }
        let block = data as u64 / block_bytes * block_bytes;
        let length = block_bytes.min(size - block) as usize;
        file.read_exact_at(&mut buffer[..length], block).context("read the volume")?;
        if buffer[..length].iter().all(|b| *b == 0) {
            let punched = unsafe { libc::fallocate(fd, libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE, block as libc::off_t, length as libc::off_t) };
            if punched != 0 {
                return Err(std::io::Error::last_os_error()).context("give a block back");
            }
            dug += length as u64;
        }
        at = block + length as u64;
    }
    file.sync_all().context("flush the volume")?;
    Ok(dug)
}

impl Computers {
    pub fn room_to_zero(&self, id: &ComputerId) -> Result<bool> {
        self.meta(id)?;
        let dir = self.computer(id);
        let meta = std::fs::metadata(dir.join(VOLUME)).context("read the volume")?;
        Ok(enough_room(available_bytes(&dir)?, meta.len(), meta.blocks().saturating_mul(STAT_BLOCK_BYTES)))
    }

    pub fn dig_holes(&self, id: &ComputerId) -> Result<u64> {
        self.meta(id)?;
        ensure!(!self.is_live(id), "computer {} is running", id.as_str());
        let path = self.computer(id).join(VOLUME);
        let file = File::options().read(true).write(true).custom_flags(libc::O_NOFOLLOW).open(&path).with_context(|| format!("open {}", path.display()))?;
        dig(&file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeroed_blocks_go_back_to_the_host_and_data_stays() {
        let path = std::env::temp_dir().join(format!("magnus-sparse-{}", std::process::id()));
        let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        let block = HOLE_BLOCK_BYTES;
        file.write_all_at(&vec![0u8; block * 40], 0).unwrap();
        file.write_all_at(b"keep me", (block * 20 + 11) as u64).unwrap();
        file.sync_all().unwrap();
        let before = file.metadata().unwrap().blocks();
        let dug = dig(&file).unwrap();
        let after = file.metadata().unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(dug, (block * 39) as u64);
        assert!(after.blocks() < before, "the blocks stayed: {before} then {}", after.blocks());
        assert_eq!(after.len(), (block * 40) as u64);
        let mut read = [0u8; 7];
        file.read_exact_at(&mut read, (block * 20 + 11) as u64).unwrap();
        assert_eq!(&read, b"keep me");
    }

    #[test]
    fn a_volume_is_filled_only_with_room_to_spare() {
        let gib = BYTES_PER_GIB;
        assert!(enough_room(20 * gib, 20 * gib, 4 * gib));
        assert!(!enough_room(19 * gib, 20 * gib, 4 * gib));
        assert!(enough_room(4 * gib, 20 * gib, 20 * gib));
    }
}
