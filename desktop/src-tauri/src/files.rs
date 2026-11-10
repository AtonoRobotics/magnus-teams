//! Which server calls deliver a file the person keeps: the app fetches only
//! these as bytes, to show or to save in their Downloads folder.
use crate::{allowed, Res};

pub(crate) fn file_path(path: &str) -> Res<()> {
    allowed(path)?;
    let route = path.split('?').next().unwrap_or_default();
    let parts: Vec<&str> = route.trim_start_matches('/').split('/').collect();
    let named = |i: usize| parts.get(i).is_some_and(|p| !p.is_empty());
    let output = parts.len() == 5 && parts[1] == "tasks" && parts[3] == "outputs";
    let library = parts.len() == 3 && parts[1] == "files" && named(2);
    let export = parts.len() == 6 && parts[1] == "chat" && parts[3] == "entries" && parts[5] == "export";
    // A cloud computer's file, or many of its files in one archive.
    let computer_file = parts.len() == 5 && parts[1] == "computers" && named(2) && parts[3] == "files" && parts[4] == "raw";
    let computer_export = parts.len() == 4 && parts[1] == "computers" && named(2) && parts[3] == "export";
    // A file Magnus made on one of the person's own computers.
    let local_file = parts.len() == 6 && parts[1] == "local" && parts[2] == "computers" && named(3) && parts[4] == "files" && parts[5] == "raw";
    if !(output || library || export || computer_file || computer_export || local_file) {
        return Err("not a file".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_calls_that_deliver_a_file_are_fetched_as_one() {
        assert!(file_path("/v1/tasks/t1/outputs/o1").is_ok());
        assert!(file_path("/v1/files/file_1").is_ok());
        assert!(file_path("/v1/chat/th1/entries/a:t1/export").is_ok());
        assert!(file_path("/v1/chat/th1/entries/a:t1/share").is_err());
        assert!(file_path("/v1/tasks/t1").is_err());
        assert!(file_path("/v1/files").is_err());
        assert!(file_path("/v1/files/file_1/info").is_err());
    }

    #[test]
    fn a_computers_files_download_one_at_a_time_or_in_bulk() {
        assert!(file_path("/v1/computers/c1/files/raw?path=%2Fworkspace%2Fa.txt").is_ok());
        assert!(file_path("/v1/computers/c1/export?paths=%2Fworkspace%2Fnotes%2F").is_ok());
        assert!(file_path("/v1/computers/c1/export").is_ok());
        assert!(file_path("/v1/local/computers/l1/files/raw?path=%2Fhome%2Fa%2Fx.md").is_ok());
        assert!(file_path("/v1/computers/c1/files?path=%2Fworkspace%2Fa.txt").is_err(), "the JSON call is not a file");
        assert!(file_path("/v1/computers//export").is_err());
        assert!(file_path("/v1/local/computers/l1/files").is_err());
        assert!(file_path("/v1/computers/c1/screen").is_err());
    }
}
