//! Where a pressed link goes. A web or `duck://` link leaves through the
//! host; a relative one names a file of the repository, resolved against
//! the folder of the document it sits in.

/// Where a link goes: the web (or a `duck://` link) through the host, or a
/// file of this repository by its full path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Web(String),
    Path(Vec<u8>),
}

/// Resolves a link's destination against `dir`, the directory of the file
/// it sits in (the root for a change's body). Each segment is read through
/// ducklink's `%XX` decoding, so `My%20File.md` names `My File.md`. An
/// anchor alone, another scheme or a path above the root goes nowhere.
pub(crate) fn target(dir: &[u8], dest: &str) -> Option<Target> {
    if ["duck://", "http://", "https://"]
        .iter()
        .any(|scheme| dest.starts_with(scheme))
    {
        return Some(Target::Web(dest.to_owned()));
    }
    let path = dest.split(['#', '?']).next().unwrap_or("");
    if path.is_empty()
        || path
            .split('/')
            .next()
            .is_some_and(|head| head.contains(':'))
    {
        return None;
    }
    let mut parts: Vec<Vec<u8>> = if path.starts_with('/') {
        Vec::new()
    } else {
        dir.split(|b| *b == b'/')
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    };
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(decoded(name)),
        }
    }
    (!parts.is_empty()).then(|| Target::Path(parts.join(&b'/')))
}

/// One path segment with its `%XX` escapes read. A segment ducklink will
/// not read as one name (a literal `(`, lowercase hex, an escaped `/` or
/// `..`) stays as written, so it can never climb or split a path.
fn decoded(segment: &str) -> Vec<u8> {
    match ducklink::tail(segment).as_deref() {
        Ok([name]) => name.as_bytes().to_vec(),
        _ => segment.as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_link_resolves_against_the_file_directory() {
        let path = |p: &str| Some(Target::Path(p.as_bytes().to_vec()));
        assert_eq!(
            target(b"docs/guide", "../README.md"),
            path("docs/README.md")
        );
        assert_eq!(target(b"docs", "./a/b.md#part"), path("docs/a/b.md"));
        assert_eq!(target(b"docs", "/src/lib.rs"), path("src/lib.rs"));
        assert_eq!(target(b"", "GUIDE.md?plain=1"), path("GUIDE.md"));
        assert_eq!(target(b"docs", "../../x.md"), None, "above the root");
        assert_eq!(target(b"docs", "#anchor"), None);
        assert_eq!(target(b"", "mailto:a@b.c"), None);
        assert_eq!(target(b"docs", "My%20File.md"), path("docs/My File.md"));
        assert_eq!(target(b"", "a%20b/%EB%B3%B4.md#x"), path("a b/\u{bcf4}.md"));
        // not one decodable name: kept as written, never a traversal
        assert_eq!(target(b"docs", "%2E%2E/x.md"), path("docs/%2E%2E/x.md"));
        assert_eq!(target(b"", "a(1).md"), path("a(1).md"));
        assert_eq!(
            target(b"docs", "https://x.example/a"),
            Some(Target::Web("https://x.example/a".into()))
        );
        assert_eq!(
            target(b"", "duck://net/forge/x"),
            Some(Target::Web("duck://net/forge/x".into()))
        );
    }
}
