//! Narrow macro expansion for the small set we have to support.

pub fn try_expand_in_export_list(s: &str, module_name: Option<&str>) -> Option<String> {
    if !s.contains('?') {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '?' {
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() || n == '_' {
                    name.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            match name.as_str() {
                "MODULE" => {
                    if let Some(m) = module_name {
                        out.push_str(m);
                    } else {
                        return None;
                    }
                }
                "LINE" => out.push('0'),
                _ => return None,
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}
