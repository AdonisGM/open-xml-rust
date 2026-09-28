//! Custom document properties of a workbook (`docProps/custom.xml`),
//! shared with the other formats through `openxml_core::properties`.

use openxml_core::properties::{CustomProperties, PropertyValue};
use openxml_core::{Error, Result};

use crate::workbook::Workbook;

impl Workbook {
    /// The custom document properties as `(name, value)` pairs; properties
    /// of unsupported types are skipped.
    ///
    /// ```
    /// use openxml_xlsx::{PropertyValue, Workbook};
    ///
    /// let mut wb = Workbook::new();
    /// wb.set_custom_property("Department", PropertyValue::Text("Sales".into()))?;
    /// let wb = Workbook::from_bytes(&wb.to_bytes()?)?;
    /// assert_eq!(wb.custom_property("Department")?, Some(PropertyValue::Text("Sales".into())));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn custom_properties(&self) -> Result<Vec<(String, PropertyValue)>> {
        let props = CustomProperties::read(self.package())?;
        Ok(props
            .iter()
            .filter_map(|(n, v)| Some((n.to_owned(), v?)))
            .collect())
    }

    /// A custom property by name.
    pub fn custom_property(&self, name: &str) -> Result<Option<PropertyValue>> {
        Ok(CustomProperties::read(self.package())?.get(name))
    }

    /// Adds or replaces a custom property.
    pub fn set_custom_property(&mut self, name: &str, value: PropertyValue) -> Result<()> {
        if name.is_empty() {
            return Err(Error::InvalidArgument("a custom property needs a name".into()));
        }
        let mut props = CustomProperties::read(self.package())?;
        props.set(name, value);
        props.write(self.package_mut())
    }

    /// Removes a custom property; returns whether it existed.
    pub fn remove_custom_property(&mut self, name: &str) -> Result<bool> {
        let mut props = CustomProperties::read(self.package())?;
        if !props.remove(name) {
            return Ok(false);
        }
        props.write(self.package_mut())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_properties_round_trip() {
        let mut wb = Workbook::new();
        wb.set_custom_property("Owner", PropertyValue::Text("Finance".into()))
            .unwrap();
        wb.set_custom_property("Year", PropertyValue::Integer(2025))
            .unwrap();
        assert!(wb.set_custom_property("", PropertyValue::Bool(true)).is_err());
        let mut back = Workbook::from_bytes(&wb.to_bytes().unwrap()).unwrap();
        assert_eq!(back.custom_properties().unwrap().len(), 2);
        assert_eq!(
            back.custom_property("Year").unwrap(),
            Some(PropertyValue::Integer(2025))
        );
        assert!(back.remove_custom_property("Year").unwrap());
        assert!(!back.remove_custom_property("Year").unwrap());
        assert_eq!(back.custom_property("Year").unwrap(), None);
    }
}
