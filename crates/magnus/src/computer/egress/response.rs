//! Decode HTTP transfer framing before inspecting a response body.
use anyhow::{bail, ensure, Context, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;

pub(super) async fn line<R: AsyncBufRead + Unpin>(from: &mut R) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    (&mut *from).take((MAX_LINE_BYTES + 1) as u64).read_until(b'\n', &mut out).await?;
    ensure!(out.len() <= MAX_LINE_BYTES && out.ends_with(b"\r\n"), "invalid or oversized HTTP line");
    Ok(out)
}

enum Framing {
    Empty,
    Close,
    Length(u64),
    Chunked { remaining: u64, delimiter: bool },
}

pub(super) struct Body {
    framing: Framing,
    copied: u64,
}

impl Body {
    pub(super) fn new(head: &str, method: &str, credential: bool) -> Result<Self> {
        let mut lengths = Vec::new();
        let mut transfer = Vec::new();
        let mut encoding = Vec::new();
        let mut lines = head.split("\r\n");
        let status = lines.next().context("missing status")?.split_whitespace().nth(1).context("missing status code")?.parse::<u16>()?;
        for line in lines.filter(|l| !l.is_empty()) {
            let (name, value) = line.split_once(':').context("malformed response header")?;
            axum::http::HeaderName::from_bytes(name.as_bytes())?;
            axum::http::HeaderValue::from_str(value.trim())?;
            match name.to_ascii_lowercase().as_str() {
                "content-length" => lengths.push(value.trim().parse::<u64>()?),
                "transfer-encoding" => transfer.extend(value.split(',').map(|s| s.trim().to_ascii_lowercase())),
                "content-encoding" => encoding.extend(value.split(',').map(|s| s.trim().to_ascii_lowercase())),
                _ => {}
            }
        }
        ensure!(!credential || encoding.iter().all(|s| s == "identity"), "a credential-bearing response must use identity content encoding");
        ensure!(transfer.is_empty() || transfer == ["chunked"], "unsupported transfer encoding");
        ensure!(lengths.len() <= 1, "ambiguous content length");
        ensure!(transfer.is_empty() || lengths.is_empty(), "conflicting response framing");
        let framing = if method == "HEAD" || status == 204 || status == 304 || (100..200).contains(&status) {
            Framing::Empty
        } else if !transfer.is_empty() {
            Framing::Chunked { remaining: 0, delimiter: false }
        } else if let Some(n) = lengths.first() {
            ensure!(*n <= MAX_RESPONSE_BYTES, "response too large");
            Framing::Length(*n)
        } else {
            Framing::Close
        };
        Ok(Self { framing, copied: 0 })
    }

    pub(super) async fn read<R: AsyncBufRead + Unpin>(&mut self, from: &mut R, buf: &mut [u8]) -> Result<usize> {
        let limit = match &mut self.framing {
            Framing::Empty => return Ok(0),
            Framing::Close => buf.len(),
            Framing::Length(remaining) => {
                if *remaining == 0 {
                    return Ok(0);
                }
                buf.len().min((*remaining).try_into().unwrap_or(usize::MAX))
            }
            Framing::Chunked { remaining, delimiter } => {
                if *remaining == 0 {
                    if *delimiter {
                        let mut crlf = [0; 2];
                        from.read_exact(&mut crlf).await?;
                        ensure!(crlf == *b"\r\n", "missing chunk delimiter");
                    }
                    let size = line(from).await?;
                    let size = std::str::from_utf8(&size)?.trim_end_matches("\r\n").split(';').next().context("missing chunk length")?;
                    ensure!(!size.is_empty() && size.bytes().all(|b| b.is_ascii_hexdigit()), "invalid chunk size");
                    *remaining = u64::from_str_radix(size, 16)?;
                    ensure!(*remaining <= MAX_RESPONSE_BYTES - self.copied, "response too large");
                    if *remaining == 0 {
                        // Trailers are consumed and not forwarded, so secrets cannot escape in them.
                        let mut total = 0;
                        loop {
                            let trailer = line(from).await?;
                            total += trailer.len();
                            ensure!(total <= MAX_LINE_BYTES, "trailers too large");
                            if trailer == b"\r\n" {
                                break;
                            }
                        }
                        self.framing = Framing::Empty;
                        return Ok(0);
                    }
                    *delimiter = true;
                }
                buf.len().min((*remaining).try_into().unwrap_or(usize::MAX))
            }
        };
        let n = from.read(&mut buf[..limit]).await?;
        if n == 0 && !matches!(self.framing, Framing::Close) {
            bail!("truncated response");
        }
        match &mut self.framing {
            Framing::Length(left) | Framing::Chunked { remaining: left, .. } => *left -= n as u64,
            _ => {}
        }
        self.copied += n as u64;
        ensure!(self.copied <= MAX_RESPONSE_BYTES, "response too large");
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::egress::surrogate::Scrubber;
    use tokio::io::BufReader;

    async fn clean(raw: &[u8], read_size: usize) -> Vec<u8> {
        let mut body = Body::new("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", "GET", true).unwrap();
        let mut input = BufReader::new(raw);
        let mut scrub = Scrubber::default();
        scrub.add("REAL-SECRET-VALUE".into(), "msur_safe".into());
        let mut buffer = vec![0; read_size];
        let mut out = Vec::new();
        loop {
            let n = body.read(&mut input, &mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            out.extend(scrub.push(&buffer[..n]));
        }
        out.extend(scrub.finish());
        out
    }

    #[tokio::test]
    async fn secret_echoes_are_scrubbed_after_dechunking_at_every_read_size() {
        for n in 1..70 {
            assert_eq!(clean(b"5\r\nREAL-\r\nC\r\nSECRET-VALUE\r\n0\r\nX-Echo: REAL-SECRET-VALUE\r\n\r\n", n).await, b"msur_safe");
            assert_eq!(clean(b"11\r\nREAL-SECRET-VALUE\r\n0\r\n\r\n", n).await, b"msur_safe");
        }
    }

    #[test]
    fn ambiguous_or_encoded_credential_responses_are_refused_before_delivery() {
        for headers in [
            "Transfer-Encoding: gzip",
            "Content-Encoding: gzip",
            "Content-Encoding: br",
            "Content-Length: 1\r\nContent-Length: 2",
            "Transfer-Encoding: chunked\r\nContent-Length: 1",
        ] {
            assert!(Body::new(&format!("HTTP/1.1 200 OK\r\n{headers}\r\n\r\n"), "GET", true).is_err());
        }
    }

    #[tokio::test]
    async fn a_truncated_chunk_is_not_a_successful_response() {
        let mut body = Body::new("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", "GET", false).unwrap();
        let mut from = BufReader::new(&b"2\r\nx"[..]);
        let mut buf = [0; 32];
        assert_eq!(body.read(&mut from, &mut buf).await.unwrap(), 1);
        assert!(body.read(&mut from, &mut buf).await.is_err());
    }
}
