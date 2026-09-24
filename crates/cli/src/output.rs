use clap::ValueEnum;

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

pub struct Context {
    pub format: OutputFormat,
}

impl Context {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    pub fn is_json(&self) -> bool {
        self.format == OutputFormat::Json
    }
}

/// Escape a string for embedding inside a double-quoted JSON string literal.
///
/// The installation half answers directly rather than through the shared
/// `Outcome` renderer (no other surface projects the `Installation` locus), so
/// its handlers assemble small JSON objects by hand. They must still route every
/// interpolated value through this — an unescaped Windows path (`C:\Users\…`)
/// or a `"` in a name otherwise produces output no JSON parser accepts.
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Render an `io::Error` naming `path` as a fixed English phrase instead of
/// `Display`'s own OS-locale message: on a non-English-locale host
/// (e.g. a Russian-locale Windows install), the raw `Display` impl calls
/// into the OS's own message table and leaks that locale's text into an
/// otherwise all-English product surface — unparseable for any consumer of
/// the output besides. `raw_os_error()` (a numeric code, locale-independent)
/// is kept for any kind not covered by a specific phrase, so nothing
/// diagnosable is lost in exchange for staying English-only.
pub(crate) fn describe_io_error(err: &std::io::Error, path: &std::path::Path) -> String {
    use std::io::ErrorKind;
    let what = match err.kind() {
        ErrorKind::NotFound => "not found".to_string(),
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        ErrorKind::AlreadyExists => "already exists".to_string(),
        ErrorKind::InvalidInput | ErrorKind::InvalidData => "invalid path or name".to_string(),
        _ => match err.raw_os_error() {
            Some(code) => format!("I/O error (os error {code})"),
            None => "I/O error".to_string(),
        },
    };
    format!("{what}: {}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{describe_io_error, json_escape};

    #[test]
    fn json_escape_makes_windows_paths_and_quotes_safe() {
        assert_eq!(json_escape(r"C:\Users\a\b.json"), r"C:\\Users\\a\\b.json");
        assert_eq!(json_escape(r#"a "quote" here"#), r#"a \"quote\" here"#);
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
        // Round-trips through a strict parser as the original string.
        let s = "tab\there \"q\" C:\\x";
        let json = format!("\"{}\"", json_escape(s));
        assert!(json.starts_with('"') && json.ends_with('"'));
    }

    #[test]
    fn describe_io_error_names_not_found_in_english() {
        let path = std::path::Path::new("C:/nope/missing.json");
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "irrelevant raw text");
        let msg = describe_io_error(&err, path);
        assert_eq!(msg, "not found: C:/nope/missing.json");
    }

    #[test]
    fn describe_io_error_names_permission_denied_in_english() {
        let path = std::path::Path::new("/root/secret");
        let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "irrelevant");
        let msg = describe_io_error(&err, path);
        assert_eq!(msg, "permission denied: /root/secret");
    }

    #[test]
    fn describe_io_error_keeps_the_numeric_os_code_for_an_unmapped_kind() {
        let path = std::path::Path::new("weird.txt");
        let err = std::io::Error::from_raw_os_error(123);
        let msg = describe_io_error(&err, path);
        assert!(
            msg.contains("123"),
            "the locale-independent numeric code must survive: {msg}"
        );
        assert!(
            msg.is_ascii(),
            "no locale-dependent text may leak through: {msg}"
        );
    }
}
