//! Structural validation of typed values against their schema.
//!
//! Generated types implement [`Validate`]: required attributes and required
//! child elements must be present. (Element order, value spaces and
//! enumerations are guaranteed by the types themselves.) For full XML Schema
//! validation of serialized output, use an external validator.

use std::fmt;

/// A validation problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Location, e.g. `/w:document/w:body/w:tbl[2]`.
    pub path: String,
    /// Description.
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Collects issues while walking a typed tree.
#[derive(Debug, Default)]
pub struct Validator {
    path: Vec<String>,
    issues: Vec<Issue>,
}

impl Validator {
    /// Starts validation at an element named `root` (e.g. `w:document`).
    pub fn new(root: &str) -> Self {
        Validator {
            path: vec![root.to_owned()],
            issues: Vec::new(),
        }
    }

    fn location(&self) -> String {
        format!("/{}", self.path.join("/"))
    }

    fn report(&mut self, message: String) {
        let path = self.location();
        self.issues.push(Issue { path, message });
    }

    /// Validates a child element named `name`, at position `index` (0-based)
    /// of its field when the field repeats.
    pub fn enter(&mut self, name: &str, index: Option<usize>, f: impl FnOnce(&mut Self)) {
        match index {
            Some(i) => self.path.push(format!("{name}[{}]", i + 1)),
            None => self.path.push(name.to_owned()),
        }
        f(self);
        self.path.pop();
    }

    /// Records a missing required attribute.
    pub fn missing_attribute(&mut self, name: &str) {
        self.report(format!("missing required attribute {name}"));
    }

    /// Records a missing required child element.
    pub fn missing_element(&mut self, name: &str) {
        self.report(format!("missing required child element {name}"));
    }

    /// Records a required attribute `ns:local` that is absent from the typed
    /// fields, distinguishing an invalid value (kept in `extra_attrs`) from a
    /// missing attribute.
    pub fn required_attribute(&mut self, ns: crate::Ns, local: &str, extra_attrs: &[crate::RawAttribute]) {
        let shown = display(ns, local);
        match extra_attrs.iter().find(|a| a.name.is(ns, local)) {
            Some(a) => self.report(format!(
                "invalid value {:?} for required attribute {shown}",
                a.value
            )),
            None => self.report(format!("missing required attribute {shown}")),
        }
    }

    /// Records a required child element `ns:local` that is absent from the
    /// typed fields, distinguishing an element whose content is invalid (kept
    /// in `extra_children`) from a missing element.
    pub fn required_element(&mut self, ns: crate::Ns, local: &str, extra_children: &[crate::ExtraChild]) {
        let shown = display(ns, local);
        if extra_children.iter().any(|c| c.element.name.is(ns, local)) {
            self.report(format!("invalid content in required child element {shown}"));
        } else {
            self.report(format!("missing required child element {shown}"));
        }
    }

    /// Records missing required content described by `what`.
    pub fn missing_content(&mut self, what: &str) {
        self.report(format!("missing required content: {what}"));
    }

    /// The issues found so far.
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// Consumes the validator and returns the issues.
    pub fn into_issues(self) -> Vec<Issue> {
        self.issues
    }
}

fn display(ns: crate::Ns, local: &str) -> String {
    if ns == crate::Ns::NONE || ns.prefix().is_empty() {
        local.to_owned()
    } else {
        format!("{}:{local}", ns.prefix())
    }
}

/// Implemented by generated schema types.
pub trait Validate {
    /// Checks this value (the element's own attributes and content).
    fn validate(&self, v: &mut Validator);
}

impl<T: Validate + ?Sized> Validate for Box<T> {
    fn validate(&self, v: &mut Validator) {
        (**self).validate(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Leaf {
        val: Option<u32>,
    }
    impl Validate for Leaf {
        fn validate(&self, v: &mut Validator) {
            if self.val.is_none() {
                v.missing_attribute("w:val");
            }
        }
    }
    struct Parent {
        leaves: Vec<Leaf>,
        required: Option<Leaf>,
    }
    impl Validate for Parent {
        fn validate(&self, v: &mut Validator) {
            for (i, l) in self.leaves.iter().enumerate() {
                v.enter("w:leaf", Some(i), |v| l.validate(v));
            }
            match &self.required {
                Some(r) => v.enter("w:req", None, |v| r.validate(v)),
                None => v.missing_element("w:req"),
            }
        }
    }

    #[test]
    fn distinguishes_invalid_from_missing() {
        let mut v = Validator::new("r");
        v.required_attribute(
            crate::Ns::W,
            "val",
            &[crate::RawAttribute::new(crate::Ns::W, "val", "x")],
        );
        v.required_attribute(crate::Ns::W, "val", &[]);
        let raw = crate::RawElement::new(crate::Ns::NONE, "t");
        v.required_element(
            crate::Ns::NONE,
            "t",
            &[crate::ExtraChild {
                anchor: 0,
                index: 0,
                element: raw,
            }],
        );
        v.required_element(crate::Ns::NONE, "t", &[]);
        let m: Vec<_> = v.issues().iter().map(|i| i.message.as_str()).collect();
        assert_eq!(
            m,
            [
                "invalid value \"x\" for required attribute w:val",
                "missing required attribute w:val",
                "invalid content in required child element t",
                "missing required child element t"
            ]
        );
    }

    #[test]
    fn collects_issues_with_paths() {
        let p = Parent {
            leaves: vec![Leaf { val: Some(1) }, Leaf { val: None }],
            required: None,
        };
        let mut v = Validator::new("w:root");
        Box::new(p).validate(&mut v);
        let issues = v.into_issues();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].path, "/w:root/w:leaf[2]");
        assert_eq!(issues[0].message, "missing required attribute w:val");
        assert_eq!(
            issues[1].to_string(),
            "/w:root: missing required child element w:req"
        );
        let mut v = Validator::new("x");
        v.missing_content("one of a, b");
        assert_eq!(v.issues().len(), 1);
    }
}
