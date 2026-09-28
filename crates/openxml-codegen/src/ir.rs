//! Intermediate representation of the code to generate.

/// Path of a generated type: `crate::<module>::<name>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypePath {
    /// Module name.
    pub module: String,
    /// Type name.
    pub name: String,
}

/// Rust types that XML Schema built-in simple types map to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    /// `String`
    String,
    /// `bool`
    Bool,
    /// `i8`
    I8,
    /// `i16`
    I16,
    /// `i32`
    I32,
    /// `i64`
    I64,
    /// `u8`
    U8,
    /// `u16`
    U16,
    /// `u32`
    U32,
    /// `u64`
    U64,
    /// `f32`
    F32,
    /// `f64`
    F64,
    /// `HexBinary`
    Hex,
    /// `Base64Binary`
    Base64,
}

impl Builtin {
    /// Maps an XML Schema built-in type name.
    pub fn from_xsd(name: &str) -> Builtin {
        match name {
            "boolean" => Builtin::Bool,
            "byte" => Builtin::I8,
            "short" => Builtin::I16,
            "int" => Builtin::I32,
            "long" | "integer" | "negativeInteger" | "nonPositiveInteger" => Builtin::I64,
            "unsignedByte" => Builtin::U8,
            "unsignedShort" => Builtin::U16,
            "unsignedInt" => Builtin::U32,
            "unsignedLong" | "nonNegativeInteger" | "positiveInteger" => Builtin::U64,
            "float" => Builtin::F32,
            "double" | "decimal" => Builtin::F64,
            "hexBinary" => Builtin::Hex,
            "base64Binary" => Builtin::Base64,
            _ => Builtin::String,
        }
    }

    /// The Rust type expression.
    pub fn rust(self) -> &'static str {
        match self {
            Builtin::String => "String",
            Builtin::Bool => "bool",
            Builtin::I8 => "i8",
            Builtin::I16 => "i16",
            Builtin::I32 => "i32",
            Builtin::I64 => "i64",
            Builtin::U8 => "u8",
            Builtin::U16 => "u16",
            Builtin::U32 => "u32",
            Builtin::U64 => "u64",
            Builtin::F32 => "f32",
            Builtin::F64 => "f64",
            Builtin::Hex => "HexBinary",
            Builtin::Base64 => "Base64Binary",
        }
    }

    /// A variant name used when the type is a union member.
    pub fn variant_name(self) -> &'static str {
        match self {
            Builtin::String => "String",
            Builtin::Bool => "Boolean",
            Builtin::I8 | Builtin::I16 | Builtin::I32 | Builtin::I64 => "Integer",
            Builtin::U8 | Builtin::U16 | Builtin::U32 | Builtin::U64 => "Unsigned",
            Builtin::F32 | Builtin::F64 => "Decimal",
            Builtin::Hex => "HexBinary",
            Builtin::Base64 => "Base64Binary",
        }
    }
}

/// The Rust type of a simple value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// A built-in type.
    Builtin(Builtin),
    /// A generated simple type.
    Named(TypePath),
}

/// A generated crate.
#[derive(Debug, Default)]
pub struct Crate {
    /// Modules (one per schema file).
    pub modules: Vec<Module>,
}

/// A generated module.
#[derive(Debug, Default)]
pub struct Module {
    /// Rust module name.
    pub name: String,
    /// Schema file name.
    pub file: String,
    /// Target namespace URI.
    pub ns_uri: String,
    /// `Ns` constant identifier of the target namespace.
    pub ns: String,
    /// Simple types.
    pub simple_types: Vec<SimpleDef>,
    /// Complex types.
    pub complex_types: Vec<ComplexDef>,
    /// Choice enums.
    pub enums: Vec<ChoiceEnum>,
    /// Global elements.
    pub globals: Vec<GlobalElement>,
}

/// A generated simple type.
#[derive(Debug, Clone)]
pub struct SimpleDef {
    /// Rust type name.
    pub name: String,
    /// Documentation lines.
    pub doc: Vec<String>,
    /// Shape.
    pub kind: SimpleKind,
}

/// Shape of a generated simple type.
#[derive(Debug, Clone)]
pub enum SimpleKind {
    /// Enumeration.
    Enum(Vec<EnumValue>),
    /// Alias of another type (restriction without enumerations).
    Alias(ValueType),
    /// Union of member types.
    Union(Vec<UnionMember>),
    /// Whitespace-separated list.
    List(ValueType),
}

/// A value of an enumeration.
#[derive(Debug, Clone)]
pub struct EnumValue {
    /// Rust variant name.
    pub variant: String,
    /// Lexical value.
    pub value: String,
    /// Documentation.
    pub doc: Option<String>,
}

/// A member of a union.
#[derive(Debug, Clone)]
pub struct UnionMember {
    /// Rust variant name.
    pub variant: String,
    /// Member type.
    pub ty: ValueType,
    /// Whether the member accepts arbitrary strings (tried last when parsing).
    pub stringy: bool,
}

/// A generated complex type.
#[derive(Debug, Clone)]
pub struct ComplexDef {
    /// Rust type name.
    pub name: String,
    /// Documentation lines.
    pub doc: Vec<String>,
    /// Attributes.
    pub attrs: Vec<AttrField>,
    /// Content.
    pub content: ContentDef,
}

/// An attribute field.
#[derive(Debug, Clone)]
pub struct AttrField {
    /// Rust field name.
    pub field: String,
    /// `Ns` identifier of the attribute name (`NONE` if unqualified).
    pub ns: String,
    /// Local name.
    pub local: String,
    /// Value type.
    pub ty: ValueType,
    /// `use="required"`.
    pub required: bool,
    /// Documentation.
    pub doc: Option<String>,
}

/// Content of a complex type.
#[derive(Debug, Clone)]
pub enum ContentDef {
    /// Attributes only.
    Empty,
    /// Simple content of the given type.
    Simple(ValueType),
    /// Mixed content (kept as raw nodes).
    Mixed,
    /// Element content.
    Fields(Vec<Field>),
}

/// An element-content field.
#[derive(Debug, Clone)]
pub struct Field {
    /// Rust field name.
    pub name: String,
    /// Shape.
    pub kind: FieldKind,
    /// Whether the schema requires at least one element for this field.
    pub required: bool,
    /// Documentation.
    pub doc: Option<String>,
}

/// Shape of an element-content field.
#[derive(Debug, Clone)]
pub enum FieldKind {
    /// A specific element (`Option<T>` or `Vec<T>`).
    Element {
        /// The element.
        elem: ElemInfo,
        /// Whether it repeats.
        multi: bool,
    },
    /// A choice among elements (`Option<Enum>` or `Vec<Enum>`).
    Choice {
        /// The enum type.
        path: TypePath,
        /// The elements the enum covers.
        elems: Vec<ElemInfo>,
        /// Wildcard accepted by the choice.
        any: Option<AnyNs>,
        /// Whether it repeats.
        multi: bool,
    },
    /// Wildcard content (`Option<RawElement>` or `Vec<RawElement>`).
    Any {
        /// Namespace constraint.
        ns: AnyNs,
        /// Whether it repeats.
        multi: bool,
    },
}

/// An element name and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElemInfo {
    /// `Ns` identifier.
    pub ns: String,
    /// Local name.
    pub local: String,
    /// Content type.
    pub ty: ElemType,
}

/// Content type of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElemType {
    /// A generated complex type.
    Complex(TypePath),
    /// A simple type.
    Simple(ValueType),
    /// Unconstrained content (`xsd:anyType`).
    Raw,
}

/// Namespace constraint of a wildcard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnyNs {
    /// `##any`
    Any,
    /// `##other`: any namespace except the given one and no-namespace.
    Other(String),
    /// Explicit list: `Ns` identifiers (`NONE` for `##local`) and foreign URIs.
    List {
        /// Known namespaces.
        known: Vec<String>,
        /// Other URIs.
        uris: Vec<String>,
    },
}

/// A choice enum.
#[derive(Debug, Clone)]
pub struct ChoiceEnum {
    /// Rust type name.
    pub name: String,
    /// Documentation lines.
    pub doc: Vec<String>,
    /// Variants.
    pub variants: Vec<Variant>,
}

/// A variant of a choice enum.
#[derive(Debug, Clone)]
pub struct Variant {
    /// Rust variant name.
    pub name: String,
    /// The element.
    pub elem: ElemInfo,
    /// Documentation.
    pub doc: Option<String>,
}

/// A global element usable as a document root.
#[derive(Debug, Clone)]
pub struct GlobalElement {
    /// Constant name.
    pub const_name: String,
    /// Local name.
    pub local: String,
    /// Content type.
    pub ty: TypePath,
    /// Namespaces to declare on the root.
    pub namespaces: Vec<String>,
    /// Documentation lines.
    pub doc: Vec<String>,
}
