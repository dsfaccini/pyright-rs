//! Node's `path.posix`, as pyright calls it through `import * as path from 'path'`
//! on a POSIX host. Ported from node `lib/path.js`. Every separator and dot is
//! ASCII, so byte offsets equal UTF-16 offsets at every slice point.

pub const SEP: &str = "/";

fn normalize_string(path: &str, allow_above_root: bool) -> String {
    let bytes = path.as_bytes();
    let mut res = String::new();
    let mut last_segment_length: usize = 0;
    let mut last_slash: isize = -1;
    let mut dots: i32 = 0;
    let mut code: u8 = 0;
    let mut i: usize = 0;
    while i <= bytes.len() {
        if i < bytes.len() {
            code = bytes[i];
        } else if code == b'/' {
            break;
        } else {
            code = b'/';
        }

        if code == b'/' {
            if last_slash == i as isize - 1 || dots == 1 {
                // NOOP
            } else if dots == 2 {
                let rb = res.as_bytes();
                if rb.len() < 2
                    || last_segment_length != 2
                    || rb[rb.len() - 1] != b'.'
                    || rb[rb.len() - 2] != b'.'
                {
                    if res.len() > 2 {
                        match res.rfind('/') {
                            None => {
                                res.clear();
                                last_segment_length = 0;
                            }
                            Some(last_slash_index) => {
                                res.truncate(last_slash_index);
                                last_segment_length = (res.len() as isize
                                    - 1
                                    - res.rfind('/').map_or(-1, |v| v as isize))
                                    as usize;
                            }
                        }
                        last_slash = i as isize;
                        dots = 0;
                        i += 1;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment_length = 0;
                        last_slash = i as isize;
                        dots = 0;
                        i += 1;
                        continue;
                    }
                }
                if allow_above_root {
                    res.push_str(if !res.is_empty() { "/.." } else { ".." });
                    last_segment_length = 2;
                }
            } else {
                let segment = &path[(last_slash + 1) as usize..i];
                if !res.is_empty() {
                    res.push('/');
                }
                res.push_str(segment);
                last_segment_length = (i as isize - last_slash - 1) as usize;
            }
            last_slash = i as isize;
            dots = 0;
        } else if code == b'.' && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
        i += 1;
    }
    res
}

/// `path.posix.normalize`.
pub fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let is_absolute = path.starts_with('/');
    let trailing_separator = path.ends_with('/');
    let mut path = normalize_string(path, !is_absolute);
    if path.is_empty() {
        if is_absolute {
            return "/".to_string();
        }
        return if trailing_separator { "./" } else { "." }.to_string();
    }
    if trailing_separator {
        path.push('/');
    }
    if is_absolute {
        format!("/{path}")
    } else {
        path
    }
}

/// `path.posix.resolve`, with `cwd` standing in for `process.cwd()`.
pub fn resolve(paths: &[&str], cwd: &str) -> String {
    let mut resolved_path = String::new();
    let mut resolved_absolute = false;
    let mut i = paths.len() as isize - 1;
    while i >= -1 && !resolved_absolute {
        let path = if i >= 0 { paths[i as usize] } else { cwd };
        i -= 1;
        if path.is_empty() {
            continue;
        }
        resolved_path = format!("{path}/{resolved_path}");
        resolved_absolute = path.starts_with('/');
    }
    let resolved_path = normalize_string(&resolved_path, !resolved_absolute);
    if resolved_absolute {
        return format!("/{resolved_path}");
    }
    if !resolved_path.is_empty() {
        resolved_path
    } else {
        ".".to_string()
    }
}

/// `path.posix.basename(path)`.
pub fn basename(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut start = 0;
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut i = bytes.len() as isize - 1;
    while i >= 0 {
        if bytes[i as usize] == b'/' {
            if !matched_slash {
                start = i as usize + 1;
                break;
            }
        } else if end == -1 {
            matched_slash = false;
            end = i + 1;
        }
        i -= 1;
    }
    if end == -1 {
        return "";
    }
    &path[start..end as usize]
}

/// `path.posix.extname(path)`.
pub fn extname(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut start_dot: isize = -1;
    let mut start_part: isize = 0;
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    let mut i = bytes.len() as isize - 1;
    while i >= 0 {
        let code = bytes[i as usize];
        if code == b'/' {
            if !matched_slash {
                start_part = i + 1;
                break;
            }
            i -= 1;
            continue;
        }
        if end == -1 {
            matched_slash = false;
            end = i + 1;
        }
        if code == b'.' {
            if start_dot == -1 {
                start_dot = i;
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot != -1 {
            pre_dot_state = -1;
        }
        i -= 1;
    }
    if start_dot == -1
        || end == -1
        || pre_dot_state == 0
        || (pre_dot_state == 1 && start_dot == end - 1 && start_dot == start_part + 1)
    {
        return "";
    }
    &path[start_dot as usize..end as usize]
}

/// `path.posix.dirname(path)`.
pub fn dirname(path: &str) -> &str {
    if path.is_empty() {
        return ".";
    }
    let bytes = path.as_bytes();
    let has_root = bytes[0] == b'/';
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut i = bytes.len() as isize - 1;
    while i >= 1 {
        if bytes[i as usize] == b'/' {
            if !matched_slash {
                end = i;
                break;
            }
        } else {
            matched_slash = false;
        }
        i -= 1;
    }
    if end == -1 {
        return if has_root { "/" } else { "." };
    }
    if has_root && end == 1 {
        return "//";
    }
    &path[..end as usize]
}

/// `path.posix.isAbsolute(path)`.
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_like_node() {
        for (input, expected) in [
            ("", "."),
            ("/a/b/../c/./d/", "/a/c/d/"),
            ("a/../../b", "../b"),
            ("/../a", "/a"),
            ("//a//b", "/a/b"),
            ("./", "./"),
            ("a/..", "."),
            ("/a/b/..", "/a"),
            ("../../x/..", "../.."),
        ] {
            assert_eq!(normalize(input), expected, "{input}");
        }
    }

    /// `tests/data/node_path.tsv`: random paths with node 26's results for
    /// normalize, basename, extname, dirname and resolve("/cwd", path).
    #[test]
    fn match_node_on_random_paths() {
        for line in include_str!("../tests/data/node_path.tsv").lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let input = f[0];
            assert_eq!(normalize(input), f[1], "normalize {input}");
            assert_eq!(basename(input), f[2], "basename {input}");
            assert_eq!(extname(input), f[3], "extname {input}");
            assert_eq!(dirname(input), f[4], "dirname {input}");
            assert_eq!(resolve(&["/cwd", input], "/"), f[5], "resolve {input}");
        }
    }

    #[test]
    fn resolve_like_node() {
        assert_eq!(resolve(&["/a/b/"], "/cwd"), "/a/b");
        assert_eq!(resolve(&["/a", "../c"], "/cwd"), "/c");
        assert_eq!(resolve(&["x"], "/cwd"), "/cwd/x");
        assert_eq!(resolve(&["/"], "/cwd"), "/");
    }

    #[test]
    fn basename_extname_like_node() {
        assert_eq!(basename("/a/b.py/"), "b.py");
        assert_eq!(basename("/"), "");
        assert_eq!(extname("/a/b.tar.gz"), ".gz");
        assert_eq!(extname("/a/.bashrc"), "");
        assert_eq!(extname("/a/b."), ".");
        assert_eq!(extname("/a/.."), "");
        assert_eq!(extname("a.py/"), ".py");
        assert_eq!(dirname("/a/b/"), "/a");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a"), ".");
    }
}
