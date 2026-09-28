//! Lowering of the schema model into the code-generation IR.
//!
//! The central decision is how an XML Schema content model becomes Rust
//! fields (see `Lowerer::layout`):
//!
//! * the top-level sequence of a type is split into its members, and nested
//!   non-repeating sequences (including references to sequence groups) are
//!   flattened into the parent;
//! * an element particle becomes `Option<T>` (at most once) or `Vec<T>`;
//! * every other particle — a choice, a repeating sequence, a reference to a
//!   choice group — becomes a field holding a *choice enum* whose variants are
//!   all the elements the particle can contain, in document order
//!   (`Option<Enum>` when at most one element can occur, `Vec<Enum>`
//!   otherwise). Enums built from named groups are shared by every type that
//!   uses the group;
//! * wildcards become raw elements.
//!
//! The resulting model is lenient when reading (element order is not
//! enforced) and canonical when writing (fields are written in schema order).

use std::collections::{BTreeSet, HashMap, HashSet};

use openxml_xml::Ns;

use crate::ir::*;
use crate::names::{camel, doc_text, screaming, snake, unique};
use crate::registry::Registry;
use crate::spec::{Entry, SpecIndex, spec_prefix};
use crate::xsd::{self, AttrItem, Content, ElementRef, Occurs, Particle, QName, SimpleBody, XML_NS, XS};

/// Rust module name for a schema file.
pub fn module_name(file: &str) -> String {
    let stem = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".xsd");
    match stem {
        "wml" => "wml".into(),
        "sml" => "sml".into(),
        "pml" => "pml".into(),
        "dml-main" => "dml".into(),
        "shared-commonSimpleTypes" => "shared_types".into(),
        "shared-documentPropertiesCustom" => "shared_custom_properties".into(),
        "shared-documentPropertiesExtended" => "shared_extended_properties".into(),
        "shared-documentPropertiesVariantTypes" => "shared_variant_types".into(),
        "vml-main" => "vml".into(),
        "vml-officeDrawing" => "vml_office".into(),
        "vml-wordprocessingDrawing" => "vml_wordprocessing".into(),
        "vml-spreadsheetDrawing" => "vml_spreadsheet".into(),
        "vml-presentationDrawing" => "vml_presentation".into(),
        other => snake(other),
    }
}

/// The `Ns` identifier of a namespace URI.
pub fn ns_ident(uri: &str) -> String {
    if uri.is_empty() {
        return "NONE".into();
    }
    if uri == XML_NS {
        return "XML".into();
    }
    match Ns::from_uri(uri) {
        Some(ns) => ns.ident().to_owned(),
        None => panic!("namespace {uri} is not part of the openxml-xml registry"),
    }
}

/// The `Ns` value for an identifier.
pub fn ns_by_ident(ident: &str) -> Ns {
    match ident {
        "NONE" => Ns::NONE,
        "OTHER" => Ns::OTHER,
        _ => Ns::all()
            .find(|n| n.ident() == ident)
            .unwrap_or_else(|| panic!("unknown Ns::{ident}")),
    }
}

/// A particle with references resolved.
#[derive(Clone, Debug)]
enum RP {
    Elem(ElemInfo, Occurs),
    Any(AnyNs, Occurs),
    Seq(Vec<RP>, Occurs),
    Choice(Vec<RP>, Occurs),
    All(Vec<RP>, Occurs),
    Group(QName, Box<RP>, Occurs),
}

impl RP {
    fn with_occurs(self, outer: Occurs) -> RP {
        match self {
            RP::Elem(e, o) => RP::Elem(e, outer.times(o)),
            RP::Any(a, o) => RP::Any(a, outer.times(o)),
            RP::Seq(i, o) => RP::Seq(i, outer.times(o)),
            RP::Choice(i, o) => RP::Choice(i, outer.times(o)),
            RP::All(i, o) => RP::All(i, outer.times(o)),
            RP::Group(q, b, o) => RP::Group(q, b, outer.times(o)),
        }
    }

    /// Maximum number of elements the particle can match (`None` = unbounded).
    fn max_count(&self) -> Option<u32> {
        let scale = |n: Option<u32>, o: Occurs| match (n, o.max) {
            (Some(0), _) | (_, Some(0)) => Some(0),
            (Some(a), Some(b)) => Some(a.saturating_mul(b)),
            _ => None,
        };
        match self {
            RP::Elem(_, o) | RP::Any(_, o) => o.max,
            RP::Seq(items, o) | RP::All(items, o) => {
                let sum = items
                    .iter()
                    .try_fold(0u32, |acc, i| i.max_count().map(|m| acc.saturating_add(m)));
                scale(sum, *o)
            }
            RP::Choice(items, o) => {
                let max = items
                    .iter()
                    .try_fold(0u32, |acc, i| i.max_count().map(|m| acc.max(m)));
                scale(max, *o)
            }
            RP::Group(_, body, o) => scale(body.max_count(), *o),
        }
    }

    /// Minimum number of elements the particle must match.
    fn min_count(&self) -> u32 {
        match self {
            RP::Elem(_, o) | RP::Any(_, o) => o.min,
            RP::Seq(items, o) | RP::All(items, o) => items
                .iter()
                .map(RP::min_count)
                .fold(0u32, u32::saturating_add)
                .saturating_mul(o.min),
            RP::Choice(items, o) => items
                .iter()
                .map(RP::min_count)
                .min()
                .unwrap_or(0)
                .saturating_mul(o.min),
            RP::Group(_, body, o) => body.min_count().saturating_mul(o.min),
        }
    }

    fn own_min(&self) -> u32 {
        match self {
            RP::Elem(_, o)
            | RP::Any(_, o)
            | RP::Seq(_, o)
            | RP::Choice(_, o)
            | RP::All(_, o)
            | RP::Group(_, _, o) => o.min,
        }
    }

    fn can_repeat(&self) -> bool {
        self.max_count().is_none_or(|m| m > 1)
    }
}

fn is_plain_sequence(rp: &RP) -> bool {
    matches!(rp, RP::Seq(_, o) | RP::All(_, o) if !o.repeats())
}

fn top_items(rp: RP) -> Vec<RP> {
    match rp {
        RP::Seq(items, o) | RP::All(items, o) if !o.repeats() => items,
        RP::Group(_, body, o) if !o.repeats() && is_plain_sequence(&body) => top_items(*body),
        other => vec![other],
    }
}

/// All elements (deduplicated, in document order) and the first wildcard of a particle.
fn flatten(rp: &RP) -> (Vec<ElemInfo>, Option<AnyNs>) {
    fn walk(rp: &RP, elems: &mut Vec<ElemInfo>, any: &mut Option<AnyNs>) {
        match rp {
            RP::Elem(e, _) => {
                if !elems.iter().any(|x| x.ns == e.ns && x.local == e.local) {
                    elems.push(e.clone());
                }
            }
            RP::Any(a, _) => {
                if any.is_none() {
                    *any = Some(a.clone());
                }
            }
            RP::Seq(items, _) | RP::Choice(items, _) | RP::All(items, _) => {
                for i in items {
                    walk(i, elems, any);
                }
            }
            RP::Group(_, body, _) => walk(body, elems, any),
        }
    }
    let mut elems = Vec::new();
    let mut any = None;
    walk(rp, &mut elems, &mut any);
    (elems, any)
}

struct AttrSpec {
    ns: String,
    local: String,
    ty: ValueType,
    required: bool,
    default: Option<String>,
}

/// Converts a registry into the generation IR.
pub struct Lowerer<'a> {
    reg: &'a Registry,
    spec: &'a SpecIndex,
    modules: Vec<Module>,
    type_names: Vec<HashSet<String>>,
    group_enums: HashMap<QName, TypePath>,
    synthesized: HashMap<(usize, String), ValueType>,
    anonymous: HashMap<(usize, String), TypePath>,
    pending: Vec<(usize, String, xsd::ComplexType)>,
}

impl<'a> Lowerer<'a> {
    fn new(reg: &'a Registry, spec: &'a SpecIndex) -> Self {
        let modules: Vec<Module> = reg
            .schemas
            .iter()
            .map(|s| Module {
                name: module_name(&s.file),
                file: s.file.rsplit('/').next().unwrap_or(&s.file).to_owned(),
                ns_uri: s.target_ns.clone(),
                ns: ns_ident(&s.target_ns),
                ..Default::default()
            })
            .collect();
        let type_names = reg
            .schemas
            .iter()
            .map(|s| {
                s.simple_types
                    .iter()
                    .map(|t| t.name.clone())
                    .chain(s.complex_types.iter().map(|t| t.name.clone()))
                    .collect()
            })
            .collect();
        Lowerer {
            reg,
            spec,
            modules,
            type_names,
            group_enums: HashMap::new(),
            synthesized: HashMap::new(),
            anonymous: HashMap::new(),
            pending: Vec::new(),
        }
    }

    fn target_ns(&self, si: usize) -> &str {
        &self.reg.schemas[si].target_ns
    }

    fn path(&self, si: usize, name: &str) -> TypePath {
        TypePath {
            module: self.modules[si].name.clone(),
            name: name.to_owned(),
        }
    }

    fn reserve_name(&mut self, si: usize, wanted: &str) -> String {
        let mut name = wanted.to_owned();
        let mut n = 2;
        while self.type_names[si].contains(&name) {
            name = format!("{wanted}{n}");
            n += 1;
        }
        self.type_names[si].insert(name.clone());
        name
    }

    // ----- simple types ------------------------------------------------------

    fn value_type(&self, q: &QName) -> ValueType {
        if q.ns == XS || q.ns == XML_NS {
            return ValueType::Builtin(Builtin::from_xsd(&q.name));
        }
        match self.reg.simple(q) {
            Some((si, t)) => ValueType::Named(self.path(si, &t.name)),
            None => panic!("unknown simple type {q:?}"),
        }
    }

    fn stringy_q(&self, q: &QName) -> bool {
        if q.ns == XS || q.ns == XML_NS {
            return Builtin::from_xsd(&q.name) == Builtin::String;
        }
        match self.reg.simple(q) {
            Some((_, t)) => self.stringy_body(&t.body),
            None => false,
        }
    }

    fn stringy_body(&self, body: &SimpleBody) -> bool {
        match body {
            SimpleBody::Restriction { enumerations, .. } if !enumerations.is_empty() => false,
            SimpleBody::Restriction { base: Some(b), .. } => self.stringy_q(b),
            SimpleBody::Restriction {
                inline_base: Some(b), ..
            } => self.stringy_body(b),
            SimpleBody::Restriction { .. } => true,
            SimpleBody::Union { .. } => false,
            SimpleBody::List { item: Some(q), .. } => self.stringy_q(q),
            SimpleBody::List { inline: Some(b), .. } => self.stringy_body(b),
            SimpleBody::List { .. } => true,
        }
    }

    fn synth_simple(&mut self, si: usize, key: &str, wanted: &str, body: &SimpleBody) -> ValueType {
        if let Some(t) = self.synthesized.get(&(si, key.to_owned())) {
            return t.clone();
        }
        let name = self.reserve_name(si, wanted);
        let ty = ValueType::Named(self.path(si, &name));
        self.synthesized.insert((si, key.to_owned()), ty.clone());
        let def = self.lower_simple(si, &name, body);
        self.modules[si].simple_types.push(def);
        ty
    }

    fn simple_doc(&self, si: usize, name: &str) -> (Vec<String>, Option<&'a Entry>) {
        let entry = spec_prefix(&self.modules[si].ns).and_then(|p| self.spec.simple_type(p, name));
        let mut doc = Vec::new();
        match entry {
            Some(e) => {
                doc.push(format!(
                    "{} (ECMA-376 Part 1 §{}).",
                    doc_text(&e.title),
                    e.section
                ));
                if !e.description.is_empty() {
                    doc.push(String::new());
                    doc.push(doc_text(&e.description));
                }
                doc.push(String::new());
                doc.push(format!("Schema type `{name}`."));
            }
            None => doc.push(format!("Schema simple type `{name}`.")),
        }
        (doc, entry)
    }

    fn lower_simple(&mut self, si: usize, name: &str, body: &SimpleBody) -> SimpleDef {
        let (doc, entry) = self.simple_doc(si, name);
        let kind = match body {
            SimpleBody::Restriction { enumerations, .. } if !enumerations.is_empty() => {
                let mut taken = Vec::new();
                let values = enumerations
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut c = camel(v);
                        if c.is_empty() {
                            c = if v.is_empty() {
                                "Empty".into()
                            } else {
                                format!("Value{}", i + 1)
                            };
                        }
                        let variant = unique(c, &mut taken, true);
                        let doc = entry
                            .and_then(|e| e.values.iter().find(|r| r.name == *v))
                            .map(|r| format!("{} — {}", doc_text(&r.title), doc_text(&r.description)));
                        EnumValue {
                            variant,
                            value: v.clone(),
                            doc,
                        }
                    })
                    .collect();
                SimpleKind::Enum(values)
            }
            SimpleBody::Restriction { base: Some(b), .. } => SimpleKind::Alias(self.value_type(b)),
            SimpleBody::Restriction {
                inline_base: Some(inner),
                ..
            } => {
                let key = format!("{name}#base");
                SimpleKind::Alias(self.synth_simple(si, &key, &format!("{name}_Base"), inner))
            }
            SimpleBody::Restriction { .. } => SimpleKind::Alias(ValueType::Builtin(Builtin::String)),
            SimpleBody::Union { members, inline } => {
                let mut taken = Vec::new();
                let mut out = Vec::new();
                for m in members {
                    let base = if m.ns == XS {
                        Builtin::from_xsd(&m.name).variant_name().to_owned()
                    } else {
                        camel(m.name.trim_start_matches("ST_"))
                    };
                    out.push(UnionMember {
                        variant: unique(base, &mut taken, true),
                        ty: self.value_type(m),
                        stringy: self.stringy_q(m),
                    });
                }
                for (i, b) in inline.iter().enumerate() {
                    let key = format!("{name}#member{i}");
                    let ty = self.synth_simple(si, &key, &format!("{name}_Member{}", i + 1), b);
                    out.push(UnionMember {
                        variant: unique(format!("Member{}", i + 1), &mut taken, true),
                        ty,
                        stringy: self.stringy_body(b),
                    });
                }
                SimpleKind::Union(out)
            }
            SimpleBody::List { item: Some(q), .. } => SimpleKind::List(self.value_type(q)),
            SimpleBody::List { inline: Some(b), .. } => {
                let key = format!("{name}#item");
                SimpleKind::List(self.synth_simple(si, &key, &format!("{name}_Item"), b))
            }
            SimpleBody::List { .. } => SimpleKind::List(ValueType::Builtin(Builtin::String)),
        };
        SimpleDef {
            name: name.to_owned(),
            doc,
            kind,
        }
    }

    // ----- particles ---------------------------------------------------------

    fn elem_type(&mut self, si: usize, e: &xsd::Element, anon_name: &str) -> ElemType {
        if let Some(t) = &e.type_name {
            if t.ns == XS {
                return if t.name == "anyType" {
                    ElemType::Raw
                } else {
                    ElemType::Simple(ValueType::Builtin(Builtin::from_xsd(&t.name)))
                };
            }
            if let Some((csi, c)) = self.reg.complex(t) {
                return ElemType::Complex(self.path(csi, &c.name));
            }
            if self.reg.simple(t).is_some() {
                return ElemType::Simple(self.value_type(t));
            }
            panic!("unknown type {t:?} of element {}", e.name);
        }
        if let Some(ct) = &e.inline_complex {
            let key = (si, anon_name.to_owned());
            if let Some(p) = self.anonymous.get(&key) {
                return ElemType::Complex(p.clone());
            }
            let name = self.reserve_name(si, anon_name);
            let path = self.path(si, &name);
            self.anonymous.insert(key, path.clone());
            self.pending.push((si, name, (**ct).clone()));
            return ElemType::Complex(path);
        }
        if let Some(sb) = &e.inline_simple {
            let key = format!("elem:{anon_name}");
            return ElemType::Simple(self.synth_simple(si, &key, anon_name, sb));
        }
        ElemType::Raw
    }

    fn elem_info(&mut self, si: usize, decl: &ElementRef, ctx: &str) -> ElemInfo {
        match decl {
            ElementRef::Ref(q) => {
                let (gsi, e) = self
                    .reg
                    .element(q)
                    .unwrap_or_else(|| panic!("unknown element {q:?}"));
                let e = e.clone();
                let ty = self.elem_type(gsi, &e, &format!("CT_{}", camel(&e.name)));
                ElemInfo {
                    ns: ns_ident(&q.ns),
                    local: q.name.clone(),
                    ty,
                }
            }
            ElementRef::Local(e) => {
                let ns = if e.qualified {
                    ns_ident(self.target_ns(si))
                } else {
                    "NONE".into()
                };
                let ty = self.elem_type(si, e, &format!("{ctx}_{}", camel(&e.name)));
                ElemInfo {
                    ns,
                    local: e.name.clone(),
                    ty,
                }
            }
        }
    }

    fn any_ns(&self, si: usize, constraint: &str) -> AnyNs {
        match constraint.trim() {
            "" | "##any" => AnyNs::Any,
            "##other" => AnyNs::Other(ns_ident(self.target_ns(si))),
            list => {
                let mut known = Vec::new();
                let mut uris = Vec::new();
                for token in list.split_whitespace() {
                    match token {
                        "##local" => known.push("NONE".to_owned()),
                        "##targetNamespace" => known.push(ns_ident(self.target_ns(si))),
                        uri => match Ns::from_uri(uri) {
                            Some(ns) => known.push(ns.ident().to_owned()),
                            None => uris.push(uri.to_owned()),
                        },
                    }
                }
                AnyNs::List { known, uris }
            }
        }
    }

    fn resolve_particle(&mut self, si: usize, p: &Particle, ctx: &str) -> Option<RP> {
        if p.occurs().max == Some(0) {
            return None;
        }
        Some(match p {
            Particle::Element { decl, occurs } => RP::Elem(self.elem_info(si, decl, ctx), *occurs),
            Particle::Any { namespace, occurs } => RP::Any(self.any_ns(si, namespace), *occurs),
            Particle::Sequence { items, occurs } => RP::Seq(
                items
                    .iter()
                    .filter_map(|i| self.resolve_particle(si, i, ctx))
                    .collect(),
                *occurs,
            ),
            Particle::Choice { items, occurs } => RP::Choice(
                items
                    .iter()
                    .filter_map(|i| self.resolve_particle(si, i, ctx))
                    .collect(),
                *occurs,
            ),
            Particle::All { items, occurs } => RP::All(
                items
                    .iter()
                    .filter_map(|i| self.resolve_particle(si, i, ctx))
                    .collect(),
                *occurs,
            ),
            Particle::Group { name, occurs } => {
                let (gsi, g) = self
                    .reg
                    .group(name)
                    .unwrap_or_else(|| panic!("unknown group {name:?}"));
                let gp = g.particle.clone()?;
                let body = self.resolve_particle(gsi, &gp, &name.name)?;
                RP::Group(name.clone(), Box::new(body), *occurs)
            }
        })
    }

    fn effective_particle(&mut self, si: usize, ct: &xsd::ComplexType, ctx: &str) -> Option<RP> {
        match &ct.content {
            Content::Empty | Content::Simple { .. } => None,
            Content::Particle(p) => self.resolve_particle(si, p, ctx),
            Content::Extension { base, particle } => {
                let base_rp = match self.reg.complex(base) {
                    Some((bsi, bct)) => {
                        let bct = bct.clone();
                        self.effective_particle(bsi, &bct, &bct.name)
                    }
                    None => None,
                };
                let ext = particle.as_ref().and_then(|p| self.resolve_particle(si, p, ctx));
                match (base_rp, ext) {
                    (Some(a), Some(b)) => Some(RP::Seq(vec![a, b], Occurs::ONE)),
                    (a, b) => a.or(b),
                }
            }
            Content::Restriction { particle, .. } => {
                particle.as_ref().and_then(|p| self.resolve_particle(si, p, ctx))
            }
        }
    }

    fn is_mixed(&self, ct: &xsd::ComplexType) -> bool {
        if ct.mixed {
            return true;
        }
        match &ct.content {
            Content::Extension { base, .. } => self.reg.complex(base).is_some_and(|(_, b)| self.is_mixed(b)),
            _ => false,
        }
    }

    fn simple_content(&self, ct: &xsd::ComplexType) -> Option<ValueType> {
        match &ct.content {
            Content::Simple { base } => match self.reg.complex(base) {
                Some((_, b)) => self.simple_content(b),
                None => Some(self.value_type(base)),
            },
            _ => None,
        }
    }

    // ----- attributes ----------------------------------------------------------

    fn collect_attrs(&mut self, si: usize, ct: &xsd::ComplexType, type_name: &str) -> Vec<AttrSpec> {
        let mut list = match &ct.content {
            Content::Extension { base, .. }
            | Content::Restriction { base, .. }
            | Content::Simple { base } => match self.reg.complex(base) {
                Some((bsi, bct)) => {
                    let bct = bct.clone();
                    self.collect_attrs(bsi, &bct, &bct.name)
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        for item in &ct.attributes {
            self.add_attr_item(si, item, type_name, &mut list);
        }
        list
    }

    fn attr_type(&mut self, si: usize, a: &xsd::Attribute, key: &str, wanted: &str) -> ValueType {
        match (&a.type_name, &a.inline_type) {
            (Some(q), _) => self.value_type(q),
            (None, Some(b)) => self.synth_simple(si, key, wanted, b),
            (None, None) => ValueType::Builtin(Builtin::String),
        }
    }

    fn add_attr_item(&mut self, si: usize, item: &AttrItem, type_name: &str, list: &mut Vec<AttrSpec>) {
        let upsert = |list: &mut Vec<AttrSpec>, spec: AttrSpec| match list
            .iter_mut()
            .find(|x| x.ns == spec.ns && x.local == spec.local)
        {
            Some(existing) => *existing = spec,
            None => list.push(spec),
        };
        match item {
            AttrItem::Local(a) => {
                let ns = if a.qualified {
                    ns_ident(self.target_ns(si))
                } else {
                    "NONE".into()
                };
                if a.prohibited {
                    list.retain(|x| !(x.ns == ns && x.local == a.name));
                    return;
                }
                let key = format!("attr:{type_name}:{}", a.name);
                let wanted = format!("{type_name}_{}", camel(&a.name));
                let ty = self.attr_type(si, a, &key, &wanted);
                upsert(
                    list,
                    AttrSpec {
                        ns,
                        local: a.name.clone(),
                        ty,
                        required: a.required,
                        default: a.default.clone(),
                    },
                );
            }
            AttrItem::Ref {
                name,
                required,
                prohibited,
                default,
            } => {
                let ns = ns_ident(&name.ns);
                if *prohibited {
                    list.retain(|x| !(x.ns == ns && x.local == name.name));
                    return;
                }
                let (ty, global_default) = if name.ns == XML_NS {
                    (ValueType::Builtin(Builtin::String), None)
                } else {
                    let (gsi, a) = self
                        .reg
                        .attribute(name)
                        .unwrap_or_else(|| panic!("unknown attribute {name:?}"));
                    let a = a.clone();
                    let key = format!("gattr:{}", a.name);
                    let wanted = format!("ST_{}Attribute", camel(&a.name));
                    (self.attr_type(gsi, &a, &key, &wanted), a.default.clone())
                };
                upsert(
                    list,
                    AttrSpec {
                        ns,
                        local: name.name.clone(),
                        ty,
                        required: *required,
                        default: default.clone().or(global_default),
                    },
                );
            }
            AttrItem::Group(q) => {
                let (gsi, g) = self
                    .reg
                    .attribute_group(q)
                    .unwrap_or_else(|| panic!("unknown attribute group {q:?}"));
                let items = g.items.clone();
                for it in &items {
                    self.add_attr_item(gsi, it, &q.name, list);
                }
            }
            AttrItem::AnyAttribute => {}
        }
    }

    // ----- layout ------------------------------------------------------------

    fn layout(&mut self, si: usize, type_name: &str, rp: RP) -> Vec<Field> {
        let mut fields = Vec::new();
        let required = rp.own_min() >= 1;
        for item in top_items(rp) {
            self.layout_item(si, type_name, item, required, &mut fields);
        }
        fields
    }

    fn layout_item(&mut self, si: usize, type_name: &str, item: RP, ctx: bool, fields: &mut Vec<Field>) {
        match item {
            RP::Elem(elem, o) => {
                let name = snake(&elem.local);
                fields.push(Field {
                    name,
                    kind: FieldKind::Element {
                        elem,
                        multi: o.repeats(),
                    },
                    required: ctx && o.min >= 1,
                    doc: None,
                });
            }
            RP::Any(ns, o) => fields.push(Field {
                name: "any".into(),
                kind: FieldKind::Any {
                    ns,
                    multi: o.repeats(),
                },
                required: ctx && o.min >= 1,
                doc: None,
            }),
            RP::Seq(items, o) | RP::All(items, o) if !o.repeats() => {
                for i in items {
                    self.layout_item(si, type_name, i, ctx && o.min >= 1, fields);
                }
            }
            RP::Group(_, body, o) if !o.repeats() && is_plain_sequence(&body) => {
                let inner = ctx && o.min >= 1 && body.own_min() >= 1;
                for i in top_items(*body) {
                    self.layout_item(si, type_name, i, inner, fields);
                }
            }
            RP::Choice(items, o) | RP::Seq(items, o) | RP::All(items, o) if items.len() == 1 => {
                let child = items.into_iter().next().expect("one item").with_occurs(o);
                self.layout_item(si, type_name, child, ctx, fields);
            }
            RP::Group(q, body, o) => {
                let rp = RP::Group(q.clone(), body, o);
                let multi = rp.can_repeat();
                let required = ctx && rp.min_count() >= 1;
                let RP::Group(_, body, _) = rp else { unreachable!() };
                let path = self.group_enum(&q, &body);
                let (elems, any) = flatten(&body);
                let name = snake(q.name.trim_start_matches("EG_"));
                fields.push(Field {
                    name,
                    kind: FieldKind::Choice {
                        path,
                        elems,
                        any,
                        multi,
                    },
                    required,
                    doc: None,
                });
            }
            other => {
                let multi = other.can_repeat();
                let required = ctx && other.min_count() >= 1;
                let (elems, any) = flatten(&other);
                if elems.is_empty() {
                    // A group of wildcards only.
                    if let Some(ns) = any {
                        fields.push(Field {
                            name: "any".into(),
                            kind: FieldKind::Any { ns, multi },
                            required,
                            doc: None,
                        });
                    }
                    return;
                }
                let name = self.reserve_name(si, &format!("{type_name}_Choice"));
                let path = self.path(si, &name);
                let variants = self.variants(&elems);
                self.modules[si].enums.push(ChoiceEnum {
                    name,
                    doc: vec![format!("A choice among the child elements of `{type_name}`.")],
                    variants,
                });
                fields.push(Field {
                    name: "choice".into(),
                    kind: FieldKind::Choice {
                        path,
                        elems,
                        any,
                        multi,
                    },
                    required,
                    doc: None,
                });
            }
        }
    }

    fn group_enum(&mut self, q: &QName, body: &RP) -> TypePath {
        if let Some(p) = self.group_enums.get(q) {
            return p.clone();
        }
        let (gsi, _) = self.reg.group(q).expect("resolved group");
        let name = self.reserve_name(gsi, &q.name);
        let path = self.path(gsi, &name);
        self.group_enums.insert(q.clone(), path.clone());
        let (elems, _) = flatten(body);
        let variants = self.variants(&elems);
        self.modules[gsi].enums.push(ChoiceEnum {
            name,
            doc: vec![format!(
                "A choice among the elements of model group `{}`.",
                q.name
            )],
            variants,
        });
        path
    }

    fn element_doc(&self, elem: &ElemInfo) -> String {
        let ns = ns_by_ident(&elem.ns);
        let qualified = if ns.prefix().is_empty() || ns == Ns::NONE {
            elem.local.clone()
        } else {
            format!("{}:{}", ns.prefix(), elem.local)
        };
        let type_name = match &elem.ty {
            ElemType::Complex(p) => Some(p.name.as_str()),
            _ => None,
        };
        let entry = spec_prefix(&elem.ns).and_then(|p| self.spec.element(p, &elem.local, type_name));
        match entry {
            Some(e) => format!("`{qualified}` — {} (§{}).", doc_text(&e.title), e.section),
            None => format!("`{qualified}` element."),
        }
    }

    fn variants(&self, elems: &[ElemInfo]) -> Vec<Variant> {
        let mut taken = vec!["Other".to_owned()];
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for e in elems {
            *counts.entry(e.local.as_str()).or_default() += 1;
        }
        elems
            .iter()
            .map(|e| {
                let base = if counts[e.local.as_str()] > 1 {
                    format!("{}{}", camel(ns_by_ident(&e.ns).prefix()), camel(&e.local))
                } else {
                    camel(&e.local)
                };
                Variant {
                    name: unique(base, &mut taken, true),
                    elem: e.clone(),
                    doc: Some(self.element_doc(e)),
                }
            })
            .collect()
    }

    // ----- complex types -----------------------------------------------------

    fn lower_complex(&mut self, si: usize, name: &str, ct: &xsd::ComplexType) {
        let attrs = self.collect_attrs(si, ct, name);
        let mut content = if self.is_mixed(ct) {
            ContentDef::Mixed
        } else if let Some(vt) = self.simple_content(ct) {
            ContentDef::Simple(vt)
        } else {
            match self.effective_particle(si, ct, name) {
                None => ContentDef::Empty,
                Some(rp) => ContentDef::Fields(self.layout(si, name, rp)),
            }
        };

        let module_ns = self.modules[si].ns.clone();
        let users: Vec<&Entry> = spec_prefix(&module_ns)
            .map(|p| self.spec.elements_of_type(p, name))
            .unwrap_or_default();

        let mut taken: Vec<String> = ["extra_attrs", "extra_children", "value", "children"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let ContentDef::Fields(fields) = &mut content {
            for f in fields.iter_mut() {
                f.name = unique(std::mem::take(&mut f.name), &mut taken, false);
                f.doc = Some(match &f.kind {
                    FieldKind::Element { elem, .. } => self.element_doc(elem),
                    FieldKind::Choice { path, .. } => {
                        format!(
                            "Child elements of choice [`{}`](crate::{}::{}).",
                            path.name, path.module, path.name
                        )
                    }
                    FieldKind::Any { .. } => "Wildcard content (`xsd:any`), kept as raw XML.".into(),
                });
            }
        }
        let attrs = attrs
            .into_iter()
            .map(|a| {
                let ns = ns_by_ident(&a.ns);
                let mut cand = snake(&a.local);
                if a.ns != "NONE" && a.ns != module_ns {
                    cand = format!("{}_{}", ns.prefix(), cand.trim_end_matches('_'));
                }
                if taken.contains(&cand) {
                    cand = format!("{cand}_attr");
                }
                let field = unique(cand, &mut taken, false);
                let shown = if a.ns == "NONE" {
                    a.local.clone()
                } else {
                    format!("{}:{}", ns.prefix(), a.local)
                };
                let row = users
                    .iter()
                    .find_map(|u| u.attributes.iter().find(|r| r.name == a.local));
                let mut doc = match row {
                    Some(r) => format!("`{shown}` — {}. {}", doc_text(&r.title), doc_text(&r.description)),
                    None => format!("`{shown}` attribute."),
                };
                if a.required {
                    doc.push_str(" Required by the schema.");
                }
                if let Some(d) = &a.default {
                    doc.push_str(&format!(" Default: `{}`.", doc_text(d)));
                }
                AttrField {
                    field,
                    ns: a.ns,
                    local: a.local,
                    ty: a.ty,
                    required: a.required,
                    doc: Some(doc),
                }
            })
            .collect();

        let mut doc = Vec::new();
        if let Some(first) = users.first() {
            doc.push(format!(
                "{} (ECMA-376 Part 1 §{}).",
                doc_text(&first.title),
                first.section
            ));
            if !first.description.is_empty() {
                doc.push(String::new());
                doc.push(doc_text(&first.description));
            }
            doc.push(String::new());
            doc.push(format!("Schema type `{name}`, used by:"));
            doc.push(String::new());
            let prefix = ns_by_ident(&module_ns).prefix();
            for u in &users {
                doc.push(format!(
                    "* `{prefix}:{}` — {} (§{})",
                    u.name,
                    doc_text(&u.title),
                    u.section
                ));
            }
        } else {
            doc.push(format!("Schema complex type `{name}`."));
        }
        self.modules[si].complex_types.push(ComplexDef {
            name: name.to_owned(),
            doc,
            attrs,
            content,
        });
    }

    fn lower_globals(&mut self, si: usize) {
        let elements = self.reg.schemas[si].elements.clone();
        let mut taken = Vec::new();
        for e in &elements {
            let ElemType::Complex(ty) = self.elem_type(si, e, &format!("CT_{}", camel(&e.name))) else {
                continue;
            };
            let const_name = unique(screaming(&e.name), &mut taken, false);
            let info = ElemInfo {
                ns: self.modules[si].ns.clone(),
                local: e.name.clone(),
                ty: ElemType::Complex(ty.clone()),
            };
            let doc = vec![self.element_doc(&info)];
            self.modules[si].globals.push(GlobalElement {
                const_name,
                local: e.name.clone(),
                ty,
                namespaces: Vec::new(),
                doc,
            });
        }
    }

    fn compute_root_namespaces(&mut self) {
        // Direct namespace usage and references of every complex type and enum.
        let mut direct: HashMap<TypePath, (BTreeSet<String>, Vec<TypePath>)> = HashMap::new();
        for m in &self.modules {
            for c in &m.complex_types {
                let mut nss = BTreeSet::new();
                let mut refs = Vec::new();
                for a in &c.attrs {
                    nss.insert(a.ns.clone());
                }
                if let ContentDef::Fields(fields) = &c.content {
                    for f in fields {
                        match &f.kind {
                            FieldKind::Element { elem, .. } => {
                                nss.insert(elem.ns.clone());
                                if let ElemType::Complex(p) = &elem.ty {
                                    refs.push(p.clone());
                                }
                            }
                            FieldKind::Choice { path, .. } => refs.push(path.clone()),
                            FieldKind::Any { .. } => {}
                        }
                    }
                }
                direct.insert(
                    TypePath {
                        module: m.name.clone(),
                        name: c.name.clone(),
                    },
                    (nss, refs),
                );
            }
            for e in &m.enums {
                let mut nss = BTreeSet::new();
                let mut refs = Vec::new();
                for v in &e.variants {
                    nss.insert(v.elem.ns.clone());
                    if let ElemType::Complex(p) = &v.elem.ty {
                        refs.push(p.clone());
                    }
                }
                direct.insert(
                    TypePath {
                        module: m.name.clone(),
                        name: e.name.clone(),
                    },
                    (nss, refs),
                );
            }
        }
        let order: HashMap<String, u16> = Ns::all().map(|n| (n.ident().to_owned(), n.index())).collect();
        for mi in 0..self.modules.len() {
            let module_ns = self.modules[mi].ns.clone();
            for gi in 0..self.modules[mi].globals.len() {
                let root = self.modules[mi].globals[gi].ty.clone();
                let mut seen = HashSet::new();
                let mut stack = vec![root];
                let mut nss = BTreeSet::new();
                while let Some(p) = stack.pop() {
                    if !seen.insert(p.clone()) {
                        continue;
                    }
                    if let Some((n, refs)) = direct.get(&p) {
                        nss.extend(n.iter().cloned());
                        stack.extend(refs.iter().cloned());
                    }
                }
                nss.remove(&module_ns);
                nss.retain(|n| !matches!(n.as_str(), "NONE" | "XML" | "XMLNS" | "OTHER"));
                let mut list: Vec<String> = nss.into_iter().collect();
                list.sort_by_key(|n| order.get(n).copied().unwrap_or(u16::MAX));
                list.insert(0, module_ns.clone());
                self.modules[mi].globals[gi].namespaces = list;
            }
        }
    }
}

/// Lowers a whole schema set.
pub fn lower(reg: &Registry, spec: &SpecIndex) -> Crate {
    let mut l = Lowerer::new(reg, spec);
    for si in 0..reg.schemas.len() {
        for st in &reg.schemas[si].simple_types {
            let def = l.lower_simple(si, &st.name, &st.body);
            l.modules[si].simple_types.push(def);
        }
    }
    for si in 0..reg.schemas.len() {
        for ct in &reg.schemas[si].complex_types {
            l.lower_complex(si, &ct.name, ct);
        }
    }
    for si in 0..reg.schemas.len() {
        l.lower_globals(si);
    }
    while let Some((si, name, ct)) = l.pending.pop() {
        l.lower_complex(si, &name, &ct);
    }
    l.compute_root_namespaces();
    for m in &mut l.modules {
        m.simple_types.sort_by(|a, b| a.name.cmp(&b.name));
        m.complex_types.sort_by(|a, b| a.name.cmp(&b.name));
        m.enums.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Crate { modules: l.modules }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xsd::parse_schema;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn lower_one(body: &str) -> Crate {
        let text = format!(
            r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns="{W}" targetNamespace="{W}"
                elementFormDefault="qualified" attributeFormDefault="qualified">{body}</xsd:schema>"#
        );
        let schema = parse_schema("wml.xsd", &text).unwrap();
        let reg = Registry::new(vec![schema]);
        lower(&reg, &SpecIndex::empty())
    }

    fn complex<'c>(c: &'c Crate, name: &str) -> &'c ComplexDef {
        c.modules[0]
            .complex_types
            .iter()
            .find(|t| t.name == name)
            .unwrap()
    }

    fn fields(def: &ComplexDef) -> Vec<(String, String)> {
        let ContentDef::Fields(fields) = &def.content else {
            panic!("{:?}", def.content)
        };
        fields
            .iter()
            .map(|f| {
                let shape = match &f.kind {
                    FieldKind::Element { elem, multi } => {
                        format!("elem {} {}", elem.local, if *multi { "*" } else { "?" })
                    }
                    FieldKind::Choice {
                        path, elems, multi, ..
                    } => format!(
                        "choice {} [{}] {}",
                        path.name,
                        elems
                            .iter()
                            .map(|e| e.local.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                        if *multi { "*" } else { "?" }
                    ),
                    FieldKind::Any { multi, .. } => format!("any {}", if *multi { "*" } else { "?" }),
                };
                (f.name.clone(), shape)
            })
            .collect()
    }

    #[test]
    fn sequence_members_become_fields_and_groups_become_enums() {
        let c = lower_one(
            r#"<xsd:complexType name="CT_P"><xsd:sequence>
                 <xsd:group ref="EG_PPr" minOccurs="0"/>
                 <xsd:group ref="EG_PContent" minOccurs="0" maxOccurs="unbounded"/>
               </xsd:sequence><xsd:attribute name="rsidR" type="xsd:hexBinary"/></xsd:complexType>
               <xsd:complexType name="CT_PPr"/><xsd:complexType name="CT_R"/>
               <xsd:group name="EG_PPr"><xsd:sequence><xsd:element name="pPr" type="CT_PPr" minOccurs="0"/></xsd:sequence></xsd:group>
               <xsd:group name="EG_PContent"><xsd:choice>
                 <xsd:element name="r" type="CT_R"/><xsd:element name="t" type="xsd:string"/>
               </xsd:choice></xsd:group>"#,
        );
        let p = complex(&c, "CT_P");
        assert_eq!(
            fields(p),
            vec![
                ("p_pr".to_owned(), "elem pPr ?".to_owned()),
                ("p_content".to_owned(), "choice EG_PContent [r,t] *".to_owned()),
            ]
        );
        assert_eq!(p.attrs[0].field, "rsid_r");
        assert_eq!(p.attrs[0].ns, "W");
        assert_eq!(p.attrs[0].ty, ValueType::Builtin(Builtin::Hex));
        let e = &c.modules[0].enums[0];
        assert_eq!(e.name, "EG_PContent");
        assert_eq!(
            e.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["R", "T"]
        );
    }

    #[test]
    fn extension_prepends_base_content_and_attributes() {
        let c = lower_one(
            r#"<xsd:complexType name="CT_Base"><xsd:sequence><xsd:element name="a" type="xsd:int"/></xsd:sequence>
                 <xsd:attribute name="x" type="xsd:int"/></xsd:complexType>
               <xsd:complexType name="CT_Derived"><xsd:complexContent><xsd:extension base="CT_Base">
                 <xsd:sequence><xsd:element name="b" type="xsd:int" maxOccurs="unbounded"/></xsd:sequence>
                 <xsd:attribute name="y" type="xsd:int"/></xsd:extension></xsd:complexContent></xsd:complexType>"#,
        );
        let d = complex(&c, "CT_Derived");
        assert_eq!(
            fields(d),
            vec![("a".into(), "elem a ?".into()), ("b".into(), "elem b *".into())]
        );
        assert_eq!(
            d.attrs.iter().map(|a| a.field.as_str()).collect::<Vec<_>>(),
            ["x", "y"]
        );
    }

    #[test]
    fn choices_single_and_repeating() {
        let c = lower_one(
            r#"<xsd:complexType name="CT_A"><xsd:choice minOccurs="0">
                 <xsd:element name="x" type="xsd:int"/><xsd:element name="y" type="xsd:int"/></xsd:choice></xsd:complexType>
               <xsd:complexType name="CT_B"><xsd:choice maxOccurs="unbounded">
                 <xsd:element name="x" type="xsd:int"/><xsd:element name="y" type="xsd:int"/></xsd:choice></xsd:complexType>
               <xsd:complexType name="CT_C"><xsd:choice>
                 <xsd:sequence><xsd:element name="x" type="xsd:int"/><xsd:element name="y" type="xsd:int"/></xsd:sequence>
                 <xsd:element name="z" type="xsd:int"/></xsd:choice></xsd:complexType>
               <xsd:complexType name="CT_D"><xsd:sequence maxOccurs="unbounded">
                 <xsd:element name="x" type="xsd:int"/></xsd:sequence></xsd:complexType>"#,
        );
        assert_eq!(
            fields(complex(&c, "CT_A")),
            vec![("choice".into(), "choice CT_A_Choice [x,y] ?".into())]
        );
        assert_eq!(
            fields(complex(&c, "CT_B")),
            vec![("choice".into(), "choice CT_B_Choice [x,y] *".into())]
        );
        assert_eq!(
            fields(complex(&c, "CT_C")),
            vec![("choice".into(), "choice CT_C_Choice [x,y,z] *".into())],
            "a sequence alternative can yield two elements"
        );
        assert_eq!(
            fields(complex(&c, "CT_D")),
            vec![("x".into(), "elem x *".into())],
            "single-item wrappers unwrap"
        );
    }

    #[test]
    fn simple_mixed_empty_and_wildcards() {
        let c = lower_one(
            r###"<xsd:complexType name="CT_Text"><xsd:simpleContent><xsd:extension base="xsd:string">
                 <xsd:attribute ref="xml:space"/></xsd:extension></xsd:simpleContent></xsd:complexType>
               <xsd:complexType name="CT_Mixed" mixed="true"><xsd:sequence><xsd:any/></xsd:sequence></xsd:complexType>
               <xsd:complexType name="CT_Empty"><xsd:attribute name="val" type="xsd:boolean" use="required" default="true"/></xsd:complexType>
               <xsd:complexType name="CT_Any"><xsd:sequence><xsd:any namespace="##other" maxOccurs="unbounded"/></xsd:sequence></xsd:complexType>"###,
        );
        let t = complex(&c, "CT_Text");
        assert!(matches!(
            t.content,
            ContentDef::Simple(ValueType::Builtin(Builtin::String))
        ));
        assert_eq!(t.attrs[0].field, "xml_space");
        assert_eq!(t.attrs[0].ns, "XML");
        assert!(matches!(complex(&c, "CT_Mixed").content, ContentDef::Mixed));
        let e = complex(&c, "CT_Empty");
        assert!(matches!(e.content, ContentDef::Empty));
        let doc = e.attrs[0].doc.as_deref().unwrap();
        assert!(
            doc.contains("Required") && doc.contains("Default: `true`"),
            "{doc}"
        );
        let a = complex(&c, "CT_Any");
        let ContentDef::Fields(f) = &a.content else {
            panic!()
        };
        assert!(matches!(&f[0].kind, FieldKind::Any { ns: AnyNs::Other(t), multi: true } if t == "W"));
    }

    #[test]
    fn simple_types() {
        let c = lower_one(
            r#"<xsd:simpleType name="ST_Jc"><xsd:restriction base="xsd:string">
                 <xsd:enumeration value="left"/><xsd:enumeration value="12pt"/><xsd:enumeration value=""/></xsd:restriction></xsd:simpleType>
               <xsd:simpleType name="ST_Alias"><xsd:restriction base="xsd:unsignedInt"><xsd:maxInclusive value="5"/></xsd:restriction></xsd:simpleType>
               <xsd:simpleType name="ST_U"><xsd:union memberTypes="ST_Str xsd:boolean ST_Jc"/></xsd:simpleType>
               <xsd:simpleType name="ST_Str"><xsd:restriction base="xsd:string"><xsd:pattern value="a"/></xsd:restriction></xsd:simpleType>
               <xsd:simpleType name="ST_L"><xsd:list itemType="xsd:int"/></xsd:simpleType>"#,
        );
        let get = |n: &str| c.modules[0].simple_types.iter().find(|t| t.name == n).unwrap();
        let SimpleKind::Enum(values) = &get("ST_Jc").kind else {
            panic!()
        };
        assert_eq!(
            values.iter().map(|v| v.variant.as_str()).collect::<Vec<_>>(),
            ["Left", "V12pt", "Empty"]
        );
        assert!(matches!(
            &get("ST_Alias").kind,
            SimpleKind::Alias(ValueType::Builtin(Builtin::U32))
        ));
        let SimpleKind::Union(members) = &get("ST_U").kind else {
            panic!()
        };
        assert_eq!(
            members
                .iter()
                .map(|m| (m.variant.as_str(), m.stringy))
                .collect::<Vec<_>>(),
            [("Str", true), ("Boolean", false), ("Jc", false)]
        );
        assert!(matches!(
            &get("ST_L").kind,
            SimpleKind::List(ValueType::Builtin(Builtin::I32))
        ));
    }

    #[test]
    fn global_elements_and_root_namespaces() {
        let c = lower_one(
            r#"<xsd:complexType name="CT_Document"><xsd:sequence><xsd:element name="body" type="CT_Body"/></xsd:sequence></xsd:complexType>
               <xsd:complexType name="CT_Body"><xsd:attribute ref="xml:lang"/></xsd:complexType>
               <xsd:element name="document" type="CT_Document"/>
               <xsd:element name="inline"><xsd:complexType><xsd:attribute name="a" type="xsd:int"/></xsd:complexType></xsd:element>
               <xsd:element name="simple" type="xsd:string"/>"#,
        );
        let globals = &c.modules[0].globals;
        assert_eq!(globals.len(), 2, "simple-typed globals are skipped");
        assert_eq!(globals[0].const_name, "DOCUMENT");
        assert_eq!(globals[0].namespaces, ["W"]);
        assert_eq!(
            globals[1].ty.name, "CT_Inline",
            "anonymous types are named after the element"
        );
        assert!(c.modules[0].complex_types.iter().any(|t| t.name == "CT_Inline"));
    }

    #[test]
    fn duplicate_names_and_keywords() {
        let c = lower_one(
            r#"<xsd:complexType name="CT_X"><xsd:sequence>
                 <xsd:element name="type" type="xsd:int"/>
                 <xsd:choice maxOccurs="unbounded"><xsd:element name="a" type="xsd:int"/><xsd:element name="b" type="xsd:int"/></xsd:choice>
                 <xsd:choice maxOccurs="unbounded"><xsd:element name="c" type="xsd:int"/><xsd:element name="d" type="xsd:int"/></xsd:choice>
               </xsd:sequence><xsd:attribute name="type" type="xsd:int"/></xsd:complexType>"#,
        );
        let x = complex(&c, "CT_X");
        let names: Vec<_> = fields(x).into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["type_", "choice", "choice_2"]);
        assert_eq!(x.attrs[0].field, "type__attr");
        let enums: Vec<_> = c.modules[0].enums.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(enums, ["CT_X_Choice", "CT_X_Choice2"]);
    }

    #[test]
    fn module_names() {
        assert_eq!(module_name("schemas/transitional/dml-main.xsd"), "dml");
        assert_eq!(module_name("dml-chart.xsd"), "dml_chart");
        assert_eq!(module_name("shared-math.xsd"), "shared_math");
        assert_eq!(module_name("vml-officeDrawing.xsd"), "vml_office");
        assert_eq!(ns_ident(""), "NONE");
        assert_eq!(ns_ident(XML_NS), "XML");
        assert_eq!(ns_by_ident("W"), Ns::W);
    }
}
