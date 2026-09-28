//! Custom and extended (application) document properties, shared by all
//! Office formats (`docProps/custom.xml`, `docProps/app.xml`; ECMA-376
//! Part 1 §15.2.12, §22.2, §22.3).

use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::shared_custom_properties::{self as custom, CT_Property, CT_Property_Choice};
use openxml_schema::shared_extended_properties as extended;

use crate::error::Result;
use crate::part::{read_part, read_related, write_part};

/// The format id Office uses for user-defined custom properties.
pub const USER_DEFINED_FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";

/// The value of a custom property.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// Text (`vt:lpwstr`).
    Text(String),
    /// Integer (`vt:i4`, or `vt:i8` when out of 32-bit range).
    Integer(i64),
    /// Floating-point number (`vt:r8`).
    Number(f64),
    /// Yes/no (`vt:bool`).
    Bool(bool),
    /// Date and time in W3CDTF form, e.g. `2024-05-01T08:00:00Z` (`vt:filetime`).
    DateTime(String),
}

impl From<&str> for PropertyValue {
    fn from(v: &str) -> Self {
        PropertyValue::Text(v.to_owned())
    }
}
impl From<String> for PropertyValue {
    fn from(v: String) -> Self {
        PropertyValue::Text(v)
    }
}
impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self {
        PropertyValue::Integer(v)
    }
}
impl From<i32> for PropertyValue {
    fn from(v: i32) -> Self {
        PropertyValue::Integer(v.into())
    }
}
impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self {
        PropertyValue::Number(v)
    }
}
impl From<bool> for PropertyValue {
    fn from(v: bool) -> Self {
        PropertyValue::Bool(v)
    }
}

fn to_choice(value: &PropertyValue) -> CT_Property_Choice {
    match value {
        PropertyValue::Text(s) => CT_Property_Choice::Lpwstr(s.clone()),
        PropertyValue::Integer(i) => match i32::try_from(*i) {
            Ok(small) => CT_Property_Choice::I4(small),
            Err(_) => CT_Property_Choice::I8(*i),
        },
        PropertyValue::Number(f) => CT_Property_Choice::R8(*f),
        PropertyValue::Bool(b) => CT_Property_Choice::Bool(*b),
        PropertyValue::DateTime(d) => CT_Property_Choice::Filetime(d.clone()),
    }
}

fn from_choice(choice: &CT_Property_Choice) -> Option<PropertyValue> {
    use CT_Property_Choice as C;
    Some(match choice {
        C::Lpwstr(s) | C::Lpstr(s) | C::Bstr(s) => PropertyValue::Text(s.clone()),
        C::I1(v) => PropertyValue::Integer((*v).into()),
        C::I2(v) => PropertyValue::Integer((*v).into()),
        C::I4(v) | C::Int(v) => PropertyValue::Integer((*v).into()),
        C::I8(v) => PropertyValue::Integer(*v),
        C::Ui1(v) => PropertyValue::Integer((*v).into()),
        C::Ui2(v) => PropertyValue::Integer((*v).into()),
        C::Ui4(v) | C::Uint(v) => PropertyValue::Integer((*v).into()),
        C::Ui8(v) => i64::try_from(*v)
            .map(PropertyValue::Integer)
            .unwrap_or(PropertyValue::Number(*v as f64)),
        C::R4(v) => PropertyValue::Number((*v).into()),
        C::R8(v) | C::Decimal(v) => PropertyValue::Number(*v),
        C::Bool(b) => PropertyValue::Bool(*b),
        C::Date(d) | C::Filetime(d) => PropertyValue::DateTime(d.clone()),
        _ => return None,
    })
}

/// User-defined document properties (`docProps/custom.xml`).
///
/// Properties of types without a [`PropertyValue`] counterpart (vectors,
/// blobs, …) are kept unchanged and reported as `None` by [`Self::get`].
///
/// ```
/// use openxml_core::properties::{CustomProperties, PropertyValue};
/// use openxml_opc::Package;
///
/// let mut pkg = Package::new();
/// let mut props = CustomProperties::read(&pkg)?;
/// props.set("Department", "Finance");
/// props.set("Reviewed", true);
/// props.write(&mut pkg)?;
/// assert_eq!(CustomProperties::read(&pkg)?.get("Reviewed"), Some(PropertyValue::Bool(true)));
/// # Ok::<(), openxml_core::Error>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomProperties {
    props: custom::CT_Properties,
}

impl CustomProperties {
    /// Reads the custom properties of a package (empty if it has none).
    pub fn read(pkg: &Package) -> Result<Self> {
        let props = read_related(
            pkg,
            None,
            rel_types::CUSTOM_PROPERTIES,
            &custom::elements::PROPERTIES,
        )?
        .map(|(_, p)| p)
        .unwrap_or_default();
        Ok(CustomProperties { props })
    }

    /// Writes the properties, creating `/docProps/custom.xml` and its package
    /// relationship when needed. An empty set removes the part.
    pub fn write(&self, pkg: &mut Package) -> Result<()> {
        let existing = pkg.related_part(None, rel_types::CUSTOM_PROPERTIES);
        if self.props.property.is_empty() {
            if let Some(name) = existing {
                pkg.remove_part(&name);
            }
            return Ok(());
        }
        let name = match existing {
            Some(n) => n,
            None => {
                let n = PartName::new("/docProps/custom.xml")?;
                pkg.set_part(n.clone(), ct::CUSTOM_PROPERTIES, Vec::new())?;
                pkg.add_relationship(None, rel_types::CUSTOM_PROPERTIES, &n)?;
                n
            }
        };
        write_part(
            pkg,
            &name,
            ct::CUSTOM_PROPERTIES,
            &custom::elements::PROPERTIES,
            &self.props,
        )
    }

    /// Number of properties.
    pub fn len(&self) -> usize {
        self.props.property.len()
    }

    /// Whether there are no properties.
    pub fn is_empty(&self) -> bool {
        self.props.property.is_empty()
    }

    /// The value of a property (`None` if absent or of an unsupported type).
    pub fn get(&self, name: &str) -> Option<PropertyValue> {
        self.props
            .property
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .and_then(|p| p.choice.as_ref())
            .and_then(from_choice)
    }

    /// Sets a property, replacing the value of an existing one.
    pub fn set(&mut self, name: &str, value: impl Into<PropertyValue>) {
        let choice = to_choice(&value.into());
        if let Some(p) = self
            .props
            .property
            .iter_mut()
            .find(|p| p.name.as_deref() == Some(name))
        {
            p.choice = Some(choice);
            return;
        }
        // Property ids start at 2 (0 and 1 are reserved) and must be unique.
        let pid = self
            .props
            .property
            .iter()
            .filter_map(|p| p.pid)
            .max()
            .unwrap_or(1)
            + 1;
        self.props.property.push(CT_Property {
            fmtid: Some(USER_DEFINED_FMTID.to_owned()),
            pid: Some(pid),
            name: Some(name.to_owned()),
            choice: Some(choice),
            ..Default::default()
        });
    }

    /// Removes a property. Returns whether it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.props.property.len();
        self.props.property.retain(|p| p.name.as_deref() != Some(name));
        before != self.props.property.len()
    }

    /// Names and values in document order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Option<PropertyValue>)> {
        self.props.property.iter().map(|p| {
            (
                p.name.as_deref().unwrap_or(""),
                p.choice.as_ref().and_then(from_choice),
            )
        })
    }

    /// The typed part.
    pub fn raw(&self) -> &custom::CT_Properties {
        &self.props
    }

    /// The typed part, mutably.
    pub fn raw_mut(&mut self) -> &mut custom::CT_Properties {
        &mut self.props
    }
}

/// Reads the extended (application) properties (`docProps/app.xml`).
pub fn read_extended_properties(pkg: &Package) -> Result<Option<extended::CT_Properties>> {
    Ok(read_related(
        pkg,
        None,
        rel_types::EXTENDED_PROPERTIES,
        &extended::elements::PROPERTIES,
    )?
    .map(|(_, p)| p))
}

/// Writes the extended properties, creating `/docProps/app.xml` when needed.
pub fn write_extended_properties(pkg: &mut Package, props: &extended::CT_Properties) -> Result<()> {
    let name = match pkg.related_part(None, rel_types::EXTENDED_PROPERTIES) {
        Some(n) => n,
        None => {
            let n = PartName::new("/docProps/app.xml")?;
            pkg.set_part(n.clone(), ct::EXTENDED_PROPERTIES, Vec::new())?;
            pkg.add_relationship(None, rel_types::EXTENDED_PROPERTIES, &n)?;
            n
        }
    };
    write_part(
        pkg,
        &name,
        ct::EXTENDED_PROPERTIES,
        &extended::elements::PROPERTIES,
        props,
    )
}

/// Reads the extended properties from a known part name (for callers that
/// already resolved it).
pub fn read_extended_part(pkg: &Package, name: &PartName) -> Result<extended::CT_Properties> {
    read_part(pkg, name, &extended::elements::PROPERTIES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_a_package() {
        let mut pkg = Package::new();
        let mut props = CustomProperties::read(&pkg).unwrap();
        assert!(props.is_empty());
        props.set("Text", "hello & <world>");
        props.set("Int", 42);
        props.set("Big", 5_000_000_000i64);
        props.set("Float", 2.5);
        props.set("Flag", false);
        props.set("When", PropertyValue::DateTime("2024-05-01T08:00:00Z".into()));
        props.write(&mut pkg).unwrap();
        let bytes = pkg.to_bytes().unwrap();
        let back = CustomProperties::read(&Package::from_bytes(&bytes).unwrap()).unwrap();
        assert_eq!(back.len(), 6);
        assert_eq!(
            back.get("Text"),
            Some(PropertyValue::Text("hello & <world>".into()))
        );
        assert_eq!(back.get("Int"), Some(PropertyValue::Integer(42)));
        assert_eq!(back.get("Big"), Some(PropertyValue::Integer(5_000_000_000)));
        assert_eq!(back.get("Float"), Some(PropertyValue::Number(2.5)));
        assert_eq!(back.get("Flag"), Some(PropertyValue::Bool(false)));
        assert_eq!(
            back.get("When"),
            Some(PropertyValue::DateTime("2024-05-01T08:00:00Z".into()))
        );
        let pids: Vec<i32> = back.raw().property.iter().filter_map(|p| p.pid).collect();
        assert_eq!(pids, [2, 3, 4, 5, 6, 7]);
        let names: Vec<&str> = back.iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Text", "Int", "Big", "Float", "Flag", "When"]);
    }

    #[test]
    fn replacing_removing_and_emptying() {
        let mut pkg = Package::new();
        let mut props = CustomProperties::default();
        props.set("A", 1);
        props.set("A", "now text");
        assert_eq!(props.len(), 1);
        props.write(&mut pkg).unwrap();
        assert_eq!(
            pkg.package_relationships()
                .by_type(rel_types::CUSTOM_PROPERTIES)
                .count(),
            1
        );
        props.set("B", true);
        props.write(&mut pkg).unwrap();
        assert_eq!(
            pkg.package_relationships()
                .by_type(rel_types::CUSTOM_PROPERTIES)
                .count(),
            1,
            "no duplicate rel"
        );
        assert!(props.remove("A"));
        assert!(!props.remove("A"));
        assert!(props.remove("B"));
        props.write(&mut pkg).unwrap();
        assert!(
            pkg.related_part(None, rel_types::CUSTOM_PROPERTIES).is_none(),
            "empty set removes the part"
        );
        assert_eq!(pkg.part_count(), 0);
    }

    #[test]
    fn unsupported_types_are_preserved() {
        let mut props = CustomProperties::default();
        props.raw_mut().property.push(CT_Property {
            fmtid: Some(USER_DEFINED_FMTID.into()),
            pid: Some(9),
            name: Some("Blob".into()),
            choice: Some(CT_Property_Choice::Blob(openxml_xml::Base64Binary(vec![1, 2, 3]))),
            ..Default::default()
        });
        props.set("Next", 1);
        assert_eq!(props.get("Blob"), None);
        assert_eq!(
            props.raw().property[1].pid,
            Some(10),
            "pids continue after the largest"
        );
        let mut pkg = Package::new();
        props.write(&mut pkg).unwrap();
        let back = CustomProperties::read(&pkg).unwrap();
        assert!(matches!(
            back.raw().property[0].choice,
            Some(CT_Property_Choice::Blob(_))
        ));
    }

    #[test]
    fn extended_properties_are_created_and_read() {
        let mut pkg = Package::new();
        assert!(read_extended_properties(&pkg).unwrap().is_none());
        let props = extended::CT_Properties {
            application: Some("openxml-rust".into()),
            company: Some("ACME".into()),
            ..Default::default()
        };
        write_extended_properties(&mut pkg, &props).unwrap();
        write_extended_properties(&mut pkg, &props).unwrap();
        let mut back = read_extended_properties(&pkg).unwrap().unwrap();
        back.extra_attrs.clear();
        assert_eq!(back, props);
        let name = pkg.related_part(None, rel_types::EXTENDED_PROPERTIES).unwrap();
        assert_eq!(
            read_extended_part(&pkg, &name).unwrap().company.as_deref(),
            Some("ACME")
        );
        assert_eq!(pkg.package_relationships().len(), 1);
    }

    #[test]
    fn value_conversions() {
        assert_eq!(
            PropertyValue::from(String::from("s")),
            PropertyValue::Text("s".into())
        );
        assert_eq!(
            from_choice(&CT_Property_Choice::Ui8(u64::MAX)),
            Some(PropertyValue::Number(u64::MAX as f64))
        );
        assert_eq!(
            from_choice(&CT_Property_Choice::R4(1.5)),
            Some(PropertyValue::Number(1.5))
        );
        assert_eq!(
            from_choice(&CT_Property_Choice::Bstr("b".into())),
            Some(PropertyValue::Text("b".into()))
        );
    }
}
