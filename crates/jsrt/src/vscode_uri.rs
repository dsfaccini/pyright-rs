//! The parts of npm `vscode-uri` 3.1.0 (`lib/umd/uri.js`, `utils.js`) that
//! pyright's `Uri.file` and `Uri.parse` call, on a POSIX host.

use crate::node_path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct URI {
    pub scheme: String,
    pub authority: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

fn reference_resolution(scheme: &str, path: String) -> String {
    match scheme {
        "https" | "http" | "file" => {
            if path.is_empty() {
                "/".to_string()
            } else if !path.starts_with('/') {
                format!("/{path}")
            } else {
                path
            }
        }
        _ => path,
    }
}

impl URI {
    /// `new Uri(scheme, authority, path, query, fragment)` in non-strict mode.
    fn new(scheme: &str, authority: String, path: String, query: String, fragment: String) -> URI {
        let scheme = if scheme.is_empty() { "file" } else { scheme };
        let path = reference_resolution(scheme, path);
        URI {
            scheme: scheme.to_string(),
            authority,
            path,
            query,
            fragment,
        }
    }

    /// `URI.file(path)`.
    pub fn file(path: &str) -> URI {
        let mut authority = String::new();
        let mut path = path.to_string();
        if path.starts_with("//") {
            match path[2..].find('/') {
                None => {
                    authority = path[2..].to_string();
                    path = "/".to_string();
                }
                Some(idx) => {
                    let idx = idx + 2;
                    authority = path[2..idx].to_string();
                    let rest = path[idx..].to_string();
                    path = if rest.is_empty() {
                        "/".to_string()
                    } else {
                        rest
                    };
                }
            }
        }
        URI::new("file", authority, path, String::new(), String::new())
    }

    /// `URI.parse(value)` in non-strict mode.
    pub fn parse(value: &str) -> URI {
        // /^(([^:/?#]+?):)?(\/\/([^/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?/
        let mut rest = value;
        let mut scheme = "";
        if let Some(end) = rest.find([':', '/', '?', '#'])
            && end > 0
            && rest.as_bytes()[end] == b':'
        {
            scheme = &rest[..end];
            rest = &rest[end + 1..];
        }
        let mut authority = "";
        if let Some(after) = rest.strip_prefix("//") {
            let end = after.find(['/', '?', '#']).unwrap_or(after.len());
            authority = &after[..end];
            rest = &after[end..];
        }
        let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
        let path = &rest[..path_end];
        rest = &rest[path_end..];
        let mut query = "";
        if let Some(after) = rest.strip_prefix('?') {
            let end = after.find('#').unwrap_or(after.len());
            query = &after[..end];
            rest = &after[end..];
        }
        let fragment = rest.strip_prefix('#').unwrap_or("");
        URI::new(
            scheme,
            percent_decode(authority),
            percent_decode(path),
            percent_decode(query),
            percent_decode(fragment),
        )
    }

    /// `uri.with({ path })`.
    pub fn with_path(&self, path: String) -> URI {
        URI::new(
            &self.scheme,
            self.authority.clone(),
            path,
            self.query.clone(),
            self.fragment.clone(),
        )
    }

    /// `uri.with({ query, fragment })`.
    pub fn with_query_fragment(&self, query: &str, fragment: &str) -> URI {
        URI::new(
            &self.scheme,
            self.authority.clone(),
            self.path.clone(),
            query.to_string(),
            fragment.to_string(),
        )
    }

    /// `uri.fsPath`: `uriToFsPath(uri, false)` on a POSIX host.
    pub fn fs_path(&self) -> String {
        let p = self.path.as_bytes();
        if !self.authority.is_empty() && self.path.len() > 1 && self.scheme == "file" {
            format!("//{}{}", self.authority, self.path)
        } else if p.len() >= 3 && p[0] == b'/' && p[1].is_ascii_alphabetic() && p[2] == b':' {
            format!("{}{}", (p[1] as char).to_ascii_lowercase(), &self.path[2..])
        } else {
            self.path.clone()
        }
    }

    /// `uri.toString()`: `_asFormatted(uri, false)`.
    pub fn to_string_encoded(&self) -> String {
        let mut res = String::new();
        if !self.scheme.is_empty() {
            res.push_str(&self.scheme);
            res.push(':');
        }
        if !self.authority.is_empty() || self.scheme == "file" {
            res.push_str("//");
        }
        if !self.authority.is_empty() {
            let mut authority = self.authority.as_str();
            if let Some(idx) = authority.find('@') {
                let userinfo = &authority[..idx];
                authority = &authority[idx + 1..];
                match userinfo.rfind(':') {
                    None => res.push_str(&encode_uri_component_fast(userinfo, false, false)),
                    Some(i) => {
                        res.push_str(&encode_uri_component_fast(&userinfo[..i], false, false));
                        res.push(':');
                        res.push_str(&encode_uri_component_fast(&userinfo[i + 1..], false, true));
                    }
                }
                res.push('@');
            }
            let authority = authority.to_lowercase();
            match authority.rfind(':') {
                None => res.push_str(&encode_uri_component_fast(&authority, false, true)),
                Some(i) => {
                    res.push_str(&encode_uri_component_fast(&authority[..i], false, true));
                    res.push_str(&authority[i..]);
                }
            }
        }
        if !self.path.is_empty() {
            let mut path = self.path.clone();
            let b = path.as_bytes();
            if b.len() >= 3 && b[0] == b'/' && b[2] == b':' {
                if b[1].is_ascii_uppercase() {
                    path = format!("/{}:{}", (b[1] + 32) as char, &path[3..]);
                }
            } else if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_uppercase() {
                path = format!("{}:{}", (b[0] + 32) as char, &path[2..]);
            }
            res.push_str(&encode_uri_component_fast(&path, true, false));
        }
        if !self.query.is_empty() {
            res.push('?');
            res.push_str(&encode_uri_component_fast(&self.query, false, false));
        }
        if !self.fragment.is_empty() {
            res.push('#');
            res.push_str(&encode_uri_component_fast(&self.fragment, false, false));
        }
        res
    }
}

/// `Utils.resolvePath(uri)` with no extra segments.
pub fn resolve_path(uri: &URI) -> URI {
    let mut path = uri.path.clone();
    let mut slash_added = false;
    if !path.starts_with('/') {
        path = format!("/{path}");
        slash_added = true;
    }
    let mut resolved_path = node_path::resolve(&[&path], "/");
    if slash_added && resolved_path.starts_with('/') && uri.authority.is_empty() {
        resolved_path = resolved_path[1..].to_string();
    }
    uri.with_path(resolved_path)
}

fn encode_table(c: char) -> Option<&'static str> {
    Some(match c {
        ':' => "%3A",
        '/' => "%2F",
        '?' => "%3F",
        '#' => "%23",
        '[' => "%5B",
        ']' => "%5D",
        '@' => "%40",
        '!' => "%21",
        '$' => "%24",
        '&' => "%26",
        '\'' => "%27",
        '(' => "%28",
        ')' => "%29",
        '*' => "%2A",
        '+' => "%2B",
        ',' => "%2C",
        ';' => "%3B",
        '=' => "%3D",
        ' ' => "%20",
        _ => return None,
    })
}

/// JS `encodeURIComponent` for text with no lone surrogates.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn encode_uri_component_fast(component: &str, is_path: bool, is_authority: bool) -> String {
    let mut res = String::new();
    let mut native_start: Option<usize> = None;
    for (pos, c) in component.char_indices() {
        let unreserved = c.is_ascii_alphanumeric()
            || matches!(c, '-' | '.' | '_' | '~')
            || (is_path && c == '/')
            || (is_authority && matches!(c, '[' | ']' | ':'));
        if unreserved {
            if let Some(start) = native_start.take() {
                res.push_str(&encode_uri_component(&component[start..pos]));
            }
            res.push(c);
        } else if let Some(escaped) = encode_table(c) {
            if let Some(start) = native_start.take() {
                res.push_str(&encode_uri_component(&component[start..pos]));
            }
            res.push_str(escaped);
        } else if native_start.is_none() {
            native_start = Some(pos);
        }
    }
    if let Some(start) = native_start {
        res.push_str(&encode_uri_component(&component[start..]));
    }
    res
}

fn decode_hex_run(run: &str) -> String {
    // `decodeURIComponentGraceful`: decode the run, or keep the first escape
    // and retry on the rest when it is not valid UTF-8.
    let bytes: Vec<u8> = (0..run.len() / 3)
        .map(|i| u8::from_str_radix(&run[i * 3 + 1..i * 3 + 3], 16).unwrap_or(0))
        .collect();
    let all_hex =
        (0..run.len() / 3).all(|i| u8::from_str_radix(&run[i * 3 + 1..i * 3 + 3], 16).is_ok());
    if all_hex && let Ok(text) = String::from_utf8(bytes) {
        return text;
    }
    if run.len() > 3 {
        format!("{}{}", &run[..3], decode_hex_run(&run[3..]))
    } else {
        run.to_string()
    }
}

/// `percentDecode`: decodes each run of `%XX` escapes.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        let is_escape = |j: usize| {
            j + 2 < b.len()
                && b[j] == b'%'
                && b[j + 1].is_ascii_alphanumeric()
                && b[j + 2].is_ascii_alphanumeric()
        };
        if is_escape(i) {
            let start = i;
            while is_escape(i) {
                i += 3;
            }
            out.push_str(&s[last..start]);
            out.push_str(&decode_hex_run(&s[start..i]));
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_round_trip() {
        let uri = resolve_path(&URI::file("/home/u/a b/c#d/../e.py"));
        assert_eq!(uri.path, "/home/u/a b/e.py");
        assert_eq!(uri.to_string_encoded(), "file:///home/u/a%20b/e.py");
        assert_eq!(uri.fs_path(), "/home/u/a b/e.py");
        assert_eq!(
            URI::file("/x/é[1]").to_string_encoded(),
            "file:///x/%C3%A9%5B1%5D"
        );
    }

    #[test]
    fn parse_file_uri() {
        let uri = URI::parse("file:///a/b%20c.py");
        assert_eq!(uri.scheme, "file");
        assert_eq!(uri.path, "/a/b c.py");
        let unc = URI::file("//server/share/x");
        assert_eq!(unc.authority, "server");
        assert_eq!(unc.fs_path(), "//server/share/x");
    }
}
