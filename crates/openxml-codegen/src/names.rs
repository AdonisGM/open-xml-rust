//! Conversion of schema names to Rust identifiers.

const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop",
    "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "self", "static",
    "struct", "super", "trait", "true", "try", "type", "typeof", "union", "unsafe", "unsized", "use",
    "virtual", "where", "while", "yield",
];

/// Converts a schema name (`rsidRPr`, `HLinks`, `cNvPr`) to `snake_case`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            continue;
        }
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if (prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower))
                && !out.ends_with('_')
            {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    let out = out.trim_end_matches('_').to_owned();
    finish_ident(out)
}

/// Converts a schema name or value to `UpperCamelCase` (for enum variants).
pub fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if upper_next {
                out.push(c.to_ascii_uppercase());
                upper_next = false;
            } else {
                out.push(c);
            }
        } else {
            upper_next = true;
        }
    }
    if out.is_empty() {
        return String::new();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    if out == "Self" {
        out.push('_');
    }
    out
}

/// Converts a name to `SCREAMING_SNAKE_CASE` (for constants).
pub fn screaming(name: &str) -> String {
    snake(name).trim_end_matches('_').to_ascii_uppercase()
}

fn finish_ident(mut s: String) -> String {
    if s.is_empty() {
        s.push_str("field");
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

/// Makes `candidate` unique among `taken` by appending `_2`, `_3`, … (or `2`, `3`
/// for camel-case names) and records it.
pub fn unique(candidate: String, taken: &mut Vec<String>, camel_case: bool) -> String {
    let mut name = candidate.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = if camel_case {
            format!("{candidate}{n}")
        } else {
            format!("{candidate}_{n}")
        };
        n += 1;
    }
    taken.push(name.clone());
    name
}

/// Escapes text for use inside a `///` doc comment.
pub fn doc_text(s: &str) -> String {
    s.replace('\n', " ")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case() {
        for (input, expected) in [
            ("rsidRPr", "rsid_r_pr"),
            ("pPr", "p_pr"),
            ("tblW", "tbl_w"),
            ("HLinks", "h_links"),
            ("HTMLText", "html_text"),
            ("cNvPr", "c_nv_pr"),
            ("SchemaRef", "schema_ref"),
            ("ID", "id"),
            ("x14ac", "x14ac"),
            ("bgColor2", "bg_color2"),
            ("dash-dot", "dash_dot"),
            ("type", "type_"),
            ("ref", "ref_"),
            ("self", "self_"),
            ("3d", "_3d"),
            ("", "field"),
            ("a__b", "a_b"),
        ] {
            assert_eq!(snake(input), expected, "{input}");
        }
    }

    #[test]
    fn camel_case() {
        for (input, expected) in [
            ("left", "Left"),
            ("bookmarkStart", "BookmarkStart"),
            ("dash-dot", "DashDot"),
            ("12pt", "V12pt"),
            ("a4", "A4"),
            ("Self", "Self_"),
            ("*", ""),
        ] {
            assert_eq!(camel(input), expected, "{input}");
        }
    }

    #[test]
    fn screaming_case() {
        assert_eq!(screaming("coreProperties"), "CORE_PROPERTIES");
        assert_eq!(screaming("type"), "TYPE");
    }

    #[test]
    fn uniqueness() {
        let mut taken = vec![];
        assert_eq!(unique("a".into(), &mut taken, false), "a");
        assert_eq!(unique("a".into(), &mut taken, false), "a_2");
        assert_eq!(unique("a".into(), &mut taken, false), "a_3");
        let mut taken = vec![];
        assert_eq!(unique("A".into(), &mut taken, true), "A");
        assert_eq!(unique("A".into(), &mut taken, true), "A2");
    }

    #[test]
    fn doc_escaping() {
        assert_eq!(doc_text("a [b] <c>\nd"), "a \\[b\\] &lt;c&gt; d");
    }
}
