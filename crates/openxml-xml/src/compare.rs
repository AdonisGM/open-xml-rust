//! Semantic comparison of XML trees.
//!
//! Used to verify that a document survives a read/write cycle: prefixes,
//! attribute order, insignificant whitespace and equivalent lexical forms of
//! the same value (`1` vs `true`, `1.0` vs `1`, `00ab` vs `00AB`) are ignored.

use std::collections::HashMap;
use std::fmt;

use crate::ns::Ns;
use crate::raw::{RawElement, RawNode};

/// Kind of a difference found by [`semantic_diff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Element names differ.
    Name,
    /// An attribute is missing from the second tree.
    MissingAttribute,
    /// An attribute appears only in the second tree.
    UnexpectedAttribute,
    /// Attribute values are not equivalent.
    Value,
    /// Character data differs.
    Text,
    /// The children differ in number or names.
    Children,
    /// Same children, different order (not necessarily an error).
    Reordered,
}

/// A difference between two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// Kind of difference.
    pub kind: DiffKind,
    /// Location, e.g. `/document/body/p[3]`.
    pub path: String,
    /// Description.
    pub message: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at {}: {}", self.kind, self.path, self.message)
    }
}

fn bool_value(s: &str) -> Option<bool> {
    match s {
        "true" | "1" | "on" | "t" => Some(true),
        "false" | "0" | "off" | "f" => Some(false),
        _ => None,
    }
}

/// Whether two lexical values denote the same value under the usual XML
/// Schema simple types.
pub fn values_equivalent(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b) = (a.trim(), b.trim());
    if a == b {
        return true;
    }
    if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>())
        && (x == y || (x.is_nan() && y.is_nan()))
    {
        return true;
    }
    if let (Some(x), Some(y)) = (bool_value(a), bool_value(b)) {
        return x == y;
    }
    if a.len() == b.len()
        && a.len() % 2 == 0
        && a.bytes().all(|c| c.is_ascii_hexdigit())
        && a.eq_ignore_ascii_case(b)
    {
        return true;
    }
    let (wa, wb): (Vec<&str>, Vec<&str>) = (a.split_whitespace().collect(), b.split_whitespace().collect());
    wa.len() > 1 && wa.len() == wb.len() && wa.iter().zip(&wb).all(|(x, y)| values_equivalent(x, y))
}

fn key(e: &RawElement) -> (String, String) {
    (e.name.uri().to_owned(), e.name.local.to_string())
}

fn direct_text(e: &RawElement) -> String {
    e.children
        .iter()
        .filter_map(|c| match c {
            RawNode::Text(t) => Some(t.as_str()),
            RawNode::Element(_) => None,
        })
        .collect()
}

fn compare(a: &RawElement, b: &RawElement, path: &str, out: &mut Vec<Difference>) {
    let mut push = |kind, message: String| {
        out.push(Difference {
            kind,
            path: path.to_owned(),
            message,
        })
    };
    if key(a) != key(b) {
        push(DiffKind::Name, format!("{:?} vs {:?}", key(a), key(b)));
        return;
    }
    // Namespace declarations and `xml:space` (a whitespace-handling hint whose
    // effect is covered by comparing the text itself) are not compared.
    let attrs = |e: &RawElement| -> HashMap<(String, String), String> {
        e.attributes
            .iter()
            .filter(|x| x.name.ns != Ns::XMLNS && !x.name.is(Ns::XML, "space"))
            .map(|x| {
                (
                    (x.name.uri().to_owned(), x.name.local.to_string()),
                    x.value.clone(),
                )
            })
            .collect()
    };
    let (aa, ba) = (attrs(a), attrs(b));
    let mut names: Vec<_> = aa.keys().chain(ba.keys()).collect();
    names.sort();
    names.dedup();
    for n in names {
        match (aa.get(n), ba.get(n)) {
            (Some(x), Some(y)) if !values_equivalent(x, y) => {
                push(DiffKind::Value, format!("@{} {x:?} vs {y:?}", n.1))
            }
            (Some(x), None) => push(DiffKind::MissingAttribute, format!("@{} = {x:?}", n.1)),
            (None, Some(y)) => push(DiffKind::UnexpectedAttribute, format!("@{} = {y:?}", n.1)),
            _ => {}
        }
    }
    let ae: Vec<&RawElement> = a.elements().collect();
    let be: Vec<&RawElement> = b.elements().collect();
    let (at, bt) = (direct_text(a), direct_text(b));
    // Whitespace-only text next to nothing is formatting; leaf values compare by equivalence.
    let text_differs = if at.trim().is_empty() && bt.trim().is_empty() {
        false
    } else if ae.is_empty() && be.is_empty() {
        !values_equivalent(&at, &bt) || at.trim() != at && at != bt
    } else {
        at.trim() != bt.trim()
    };
    if text_differs {
        push(DiffKind::Text, format!("{at:?} vs {bt:?}"));
    }
    let child_path = |e: &RawElement, i: usize| format!("{path}/{}[{}]", e.name.local, i + 1);
    let same_order = ae.len() == be.len() && ae.iter().zip(&be).all(|(x, y)| key(x) == key(y));
    if same_order {
        for (i, (x, y)) in ae.iter().zip(&be).enumerate() {
            compare(x, y, &child_path(x, i), out);
        }
        return;
    }
    // Compare as multisets grouped by name, keeping the relative order within a name.
    let group = |v: &[&'_ RawElement]| {
        let mut m: Vec<((String, String), Vec<usize>)> = Vec::new();
        for (i, e) in v.iter().enumerate() {
            let k = key(e);
            match m.iter_mut().find(|(g, _)| *g == k) {
                Some((_, idx)) => idx.push(i),
                None => m.push((k, vec![i])),
            }
        }
        m
    };
    let (ga, gb) = (group(&ae), group(&be));
    let mut consistent = ga.len() == gb.len();
    for (k, ia) in &ga {
        match gb.iter().find(|(g, _)| g == k) {
            Some((_, ib)) if ib.len() == ia.len() => {
                for (&i, &j) in ia.iter().zip(ib) {
                    compare(ae[i], be[j], &child_path(ae[i], i), out);
                }
            }
            Some((_, ib)) => {
                consistent = false;
                out.push(Difference {
                    kind: DiffKind::Children,
                    path: path.to_owned(),
                    message: format!("{} <{}> vs {}", ia.len(), k.1, ib.len()),
                });
            }
            None => {
                consistent = false;
                out.push(Difference {
                    kind: DiffKind::Children,
                    path: path.to_owned(),
                    message: format!("<{}> missing ({}x)", k.1, ia.len()),
                });
            }
        }
    }
    for (k, ib) in &gb {
        if !ga.iter().any(|(g, _)| g == k) {
            consistent = false;
            out.push(Difference {
                kind: DiffKind::Children,
                path: path.to_owned(),
                message: format!("unexpected <{}> ({}x)", k.1, ib.len()),
            });
        }
    }
    if consistent {
        out.push(Difference {
            kind: DiffKind::Reordered,
            path: path.to_owned(),
            message: "children appear in a different order".into(),
        });
    }
}

/// Lists the semantic differences between two element trees.
pub fn semantic_diff(expected: &RawElement, actual: &RawElement) -> Vec<Difference> {
    let mut out = Vec::new();
    compare(expected, actual, &format!("/{}", expected.name.local), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(a: &str, b: &str) -> Vec<Difference> {
        semantic_diff(&RawElement::parse(a).unwrap(), &RawElement::parse(b).unwrap())
    }

    #[test]
    fn equivalent_values() {
        for (a, b) in [
            ("1", "true"),
            ("0", "false"),
            ("off", "0"),
            ("1.0", "1"),
            ("+7", "7"),
            ("1e3", "1000"),
            ("00ab", "00AB"),
            (" x ", "x"),
            ("1  2", "1 2"),
            ("1.50 2", "1.5 2.0"),
        ] {
            assert!(values_equivalent(a, b), "{a:?} ~ {b:?}");
        }
        for (a, b) in [
            ("1", "2"),
            ("true", "false"),
            ("abc", "ABC"),
            ("a", "A"),
            ("a b", "a c"),
            ("x", ""),
        ] {
            assert!(!values_equivalent(a, b), "{a:?} !~ {b:?}");
        }
    }

    #[test]
    fn ignores_prefixes_attribute_order_and_whitespace() {
        let a = r#"<w:p xmlns:w="urn:w" w:a="1" w:b="x">
            <w:r/>
        </w:p>"#;
        let b = r#"<q:p xmlns:q="urn:w" q:b="x" q:a="true"><q:r/></q:p>"#;
        assert!(diff(a, b).is_empty(), "{:?}", diff(a, b));
    }

    #[test]
    fn reports_differences() {
        let d = diff(
            r#"<a x="1" y="2"><b>t</b><c/></a>"#,
            r#"<a x="3" z="4"><b>u</b></a>"#,
        );
        let kinds: Vec<_> = d.iter().map(|d| d.kind).collect();
        assert!(kinds.contains(&DiffKind::Value));
        assert!(kinds.contains(&DiffKind::MissingAttribute));
        assert!(kinds.contains(&DiffKind::UnexpectedAttribute));
        assert!(kinds.contains(&DiffKind::Text));
        assert!(kinds.contains(&DiffKind::Children));
        assert!(d.iter().any(|x| x.path == "/a/b[1]"), "{d:?}");
        assert!(d[0].to_string().contains(" at /a"));
        assert_eq!(diff("<a/>", "<b/>")[0].kind, DiffKind::Name);
        assert_eq!(diff("<a><b/></a>", "<a><b/><x/></a>")[0].kind, DiffKind::Children);
    }

    #[test]
    fn xml_space_is_not_compared() {
        assert!(diff(r#"<t xml:space="preserve">a</t>"#, "<t>a</t>").is_empty());
    }

    #[test]
    fn reordering_is_reported_separately() {
        let d = diff("<a><b/><c/></a>", "<a><c/><b/></a>");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DiffKind::Reordered);
    }

    #[test]
    fn text_comparison() {
        assert_eq!(
            diff("<t> a </t>", "<t>a</t>")[0].kind,
            DiffKind::Text,
            "significant spaces"
        );
        assert!(diff("<t>\n<x/>\n</t>", "<t><x/></t>").is_empty(), "indentation");
        assert!(diff("<t>\n   </t>", "<t/>").is_empty(), "whitespace-only leaf");
        assert!(
            diff("<b>0</b>", "<b>false</b>").is_empty(),
            "equivalent leaf values"
        );
        assert_eq!(diff("<t>a</t>", "<t>b</t>")[0].kind, DiffKind::Text);
    }
}
