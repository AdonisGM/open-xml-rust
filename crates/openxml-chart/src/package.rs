//! Adding charts to a package and referencing them from drawings.

use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::{dml, dml_chart};
use openxml_xml::{Ns, RawElement};

use crate::build::DataSource;
use crate::spec::Chart;
use crate::workbook::embedded_workbook;

/// `uri` of `a:graphicData` holding a chart reference.
pub const CHART_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// Content type of an embedded `.xlsx` package.
pub const XLSX_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// How the chart keeps its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartData {
    /// Embed a workbook with the data (Word and PowerPoint: enables "Edit Data").
    EmbeddedWorkbook,
    /// Keep the values in the chart only; series with spreadsheet references
    /// (workbooks) point at the host's own cells.
    InChart,
}

/// A chart added to a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertedChart {
    /// The chart part, e.g. `/word/charts/chart1.xml`.
    pub part: PartName,
    /// Relationship id from the source part to the chart part.
    pub rel_id: String,
    /// The embedded workbook part, if any.
    pub workbook: Option<PartName>,
}

/// Adds `chart` as a new chart part under `host_dir` (e.g. `/word`, `/ppt`,
/// `/xl`) and relates it from `source` (the part whose drawing shows it).
pub fn insert_chart(
    pkg: &mut Package,
    source: &PartName,
    host_dir: &str,
    chart: &Chart,
    data: ChartData,
) -> Result<InsertedChart> {
    if !pkg.contains(source) {
        return Err(Error::MissingPart(source.to_string()));
    }
    let dir = host_dir.trim_end_matches('/');
    let part = pkg.next_part_name(&format!("{dir}/charts/chart{{}}.xml"))?;
    // The chart XML references the workbook relationship, so create the part
    // first (empty), relate the workbook, then write the XML.
    pkg.add_part(part.clone(), ct::CHART, Vec::new())?;
    let (workbook, source_mode) = match data {
        ChartData::EmbeddedWorkbook => {
            let wb = pkg.next_part_name(&format!("{dir}/embeddings/Microsoft_Excel_Worksheet{{}}.xlsx"))?;
            pkg.add_part(wb.clone(), XLSX_CONTENT_TYPE, embedded_workbook(chart))?;
            let id = pkg.add_relationship(Some(&part), rel_types::PACKAGE, &wb)?;
            (Some(wb), Some(id))
        }
        ChartData::InChart => (None, None),
    };
    let source_kind = match &source_mode {
        Some(id) => DataSource::Embedded(id),
        None => DataSource::Literal,
    };
    let xml = dml_chart::elements::CHART_SPACE.to_bytes(&chart.to_chart_space(source_kind));
    pkg.set_part_data(&part, xml)?;
    let rel_id = pkg.add_relationship(Some(source), rel_types::CHART, &part)?;
    Ok(InsertedChart {
        part,
        rel_id,
        workbook,
    })
}

/// The `c:chart` element that references a chart part from a drawing.
pub fn chart_reference(rel_id: &str) -> RawElement {
    let reference = dml_chart::CT_RelId {
        r_id: Some(rel_id.to_owned()),
        ..Default::default()
    };
    RawElement::from_typed(&reference, Ns::C, "chart")
}

/// `a:graphicData` showing the chart related with `rel_id`; hosts wrap it in
/// `wp:inline`/`wp:anchor` (Word), `p:graphicFrame` (PowerPoint) or
/// `xdr:graphicFrame` (Excel).
pub fn graphic_data(rel_id: &str) -> dml::CT_GraphicalObjectData {
    dml::CT_GraphicalObjectData {
        uri: Some(CHART_URI.to_owned()),
        any: vec![chart_reference(rel_id)],
        ..Default::default()
    }
}

/// The relationship id of the chart referenced by a graphicData, if it holds one.
pub fn chart_rel_id(data: &dml::CT_GraphicalObjectData) -> Option<String> {
    if data.uri.as_deref() != Some(CHART_URI) {
        return None;
    }
    data.any
        .iter()
        .find(|e| e.name.is(Ns::C, "chart"))
        .and_then(|e| e.attr(Ns::R, "id"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ChartKind, Series};

    #[test]
    fn graphic_data_round_trip() {
        let data = graphic_data("rId7");
        assert_eq!(chart_rel_id(&data).as_deref(), Some("rId7"));
        let xml = openxml_xml::RawElement::from_typed(&data, Ns::A, "graphicData").to_xml();
        assert!(
            xml.contains(r#"uri="http://schemas.openxmlformats.org/drawingml/2006/chart""#),
            "{xml}"
        );
        assert!(xml.contains(r#"r:id="rId7""#));
        let other = dml::CT_GraphicalObjectData {
            uri: Some("urn:x".into()),
            ..Default::default()
        };
        assert_eq!(chart_rel_id(&other), None);
    }

    #[test]
    fn inserting_requires_the_source_part() {
        let mut pkg = Package::new();
        let source = PartName::new("/ppt/slides/slide1.xml").unwrap();
        let chart = Chart::new(ChartKind::Pie).series(Series::new("s", [1.0]));
        assert!(insert_chart(&mut pkg, &source, "/ppt", &chart, ChartData::InChart).is_err());
    }
}
