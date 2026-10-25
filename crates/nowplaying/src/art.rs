//! Artwork URLs as players publish them.

use std::path::PathBuf;

/// The local file a `file://` URL names, with percent-escapes decoded.
/// Anything else (`https://`, `data:`, empty) is not a file.
pub(crate) fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // `file://host/path` names a host; only the local forms are files here.
    let path = match rest.strip_prefix("localhost/") {
        Some(p) => format!("/{p}"),
        None if rest.starts_with('/') => rest.to_owned(),
        None => return None,
    };
    Some(PathBuf::from(percent_decode(&path)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = bytes[i] == b'%' && i + 3 <= bytes.len();
        if let Some(byte) = escaped.then(|| hex_pair(&bytes[i + 1..i + 3])).flatten() {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_pair(pair: &[u8]) -> Option<u8> {
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    Some(digit(pair[0])? << 4 | digit(pair[1])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_file_url() {
        assert_eq!(
            file_path("file:///tmp/.org.chromium.Chromium.CfSZuP"),
            Some(PathBuf::from("/tmp/.org.chromium.Chromium.CfSZuP"))
        );
    }

    #[test]
    fn escapes_are_decoded() {
        assert_eq!(
            file_path("file:///home/me/Music/Cover%20Art%20%2B%20more.jpg"),
            Some(PathBuf::from("/home/me/Music/Cover Art + more.jpg"))
        );
        assert_eq!(file_path("file:///a%2"), Some(PathBuf::from("/a%2")));
        assert_eq!(file_path("file:///a%zz"), Some(PathBuf::from("/a%zz")));
        assert_eq!(file_path("file:///%C3%A9"), Some(PathBuf::from("/é")));
    }

    #[test]
    fn localhost_form() {
        assert_eq!(
            file_path("file://localhost/x.png"),
            Some(PathBuf::from("/x.png"))
        );
    }

    #[test]
    fn not_files() {
        assert_eq!(file_path(""), None);
        assert_eq!(file_path("https://i.scdn.co/image/abc"), None);
        assert_eq!(file_path("data:image/png;base64,AAAA"), None);
        assert_eq!(file_path("file://nas/share/x.png"), None);
    }
}
