//! The server gzips its JSON answers for a client that asks (2026-11-28):
//! lists compress about ten to one, and this app often reaches the server
//! over Tailscale. The app asks on its API calls and reads the answer back
//! here; an answer that is not gzipped is read as it is.

use std::io::Read;

/// What the app's API calls accept.
pub const ACCEPT: &str = "gzip";

/// A response's text, ungzipped when the server gzipped it.
pub async fn text(r: reqwest::Response) -> String {
    let gzipped = r.headers().get(reqwest::header::CONTENT_ENCODING).is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"gzip"));
    let bytes = r.bytes().await.unwrap_or_default();
    decode(&bytes, gzipped)
}

/// The text of `bytes`; empty when gzipped bytes do not read back, as an
/// answer that could not be read at all.
pub fn decode(bytes: &[u8], gzipped: bool) -> String {
    if !gzipped {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut s = String::new();
    match flate2::read::GzDecoder::new(bytes).read_to_string(&mut s) {
        Ok(_) => s,
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;
    use std::io::Write;

    #[test]
    fn a_gzipped_answer_reads_back_and_a_plain_one_as_it_is() {
        let json = r#"{"tasks":[{"task_id":"task_1","title":"Write the weekly report"}]}"#;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(json.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(decode(&gz, true), json);
        assert_eq!(decode(json.as_bytes(), false), json);
        assert_eq!(decode(b"not gzip", true), "", "unreadable gzip is no answer");
    }
}
