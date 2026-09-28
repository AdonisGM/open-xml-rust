//! Charts on slides (DrawingML charts from `openxml-chart`).

use openxml_chart::{Chart, ChartData, ChartInfo, chart_rel_id, graphic_data, insert_chart, read_chart};
use openxml_core::{Error, Length, Result};
use openxml_schema::pml;

use crate::presentation::Presentation;
use crate::slide::SlideMut;

impl SlideMut<'_> {
    /// Adds a chart at the given position. The chart data is stored in an
    /// embedded workbook, so PowerPoint's "Edit Data" works. Returns the
    /// shape id of the graphic frame.
    ///
    /// ```
    /// use openxml_chart::{Chart, ChartKind, Series};
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
    /// slide.set_title("Revenue")?;
    /// let chart = Chart::new(ChartKind::Column)
    ///     .categories(["Q1", "Q2"])
    ///     .series(Series::new("2024", [10.0, 12.0]));
    /// slide.add_chart(&chart, Length::cm(2.0), Length::cm(4.0), Length::cm(20.0), Length::cm(12.0))?;
    /// let bytes = deck.to_bytes()?;
    /// let deck = Presentation::from_bytes(&bytes)?;
    /// assert_eq!(deck.slide_charts(0)?[0].plots[0].series[0].values, [Some(10.0), Some(12.0)]);
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_chart(&mut self, chart: &Chart, x: Length, y: Length, w: Length, h: Length) -> Result<u32> {
        let slide_part = self.part_name().clone();
        let inserted = insert_chart(
            &mut self.pres.package,
            &slide_part,
            "/ppt",
            chart,
            ChartData::EmbeddedWorkbook,
        )?;
        let name = format!("Chart {}", self.next_id());
        Ok(self.add_graphic_frame(&name, graphic_data(&inserted.rel_id), x, y, w, h))
    }
}

fn chart_ids(tree: &pml::CT_GroupShape, out: &mut Vec<String>) {
    for item in &tree.choice {
        match item {
            pml::CT_GroupShape_Choice::GraphicFrame(frame) => {
                if let Some(id) = frame
                    .graphic
                    .as_ref()
                    .and_then(|g| g.graphic_data.as_ref())
                    .and_then(|d| chart_rel_id(d))
                {
                    out.push(id);
                }
            }
            pml::CT_GroupShape_Choice::GrpSp(group) => chart_ids(group, out),
            _ => {}
        }
    }
}

impl Presentation {
    /// The charts of slide `index`, in drawing order (including charts inside groups).
    pub fn slide_charts(&self, index: usize) -> Result<Vec<ChartInfo>> {
        let slide = self
            .slides
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("slide {index}")))?;
        let mut ids = Vec::new();
        if let Some(tree) = slide.data.c_sld.as_ref().and_then(|c| c.sp_tree.as_ref()) {
            chart_ids(tree, &mut ids);
        }
        ids.iter()
            .map(|id| {
                let part = self
                    .package
                    .relationship_target(Some(&slide.part), id)
                    .ok_or_else(|| {
                        Error::InvalidDocument(format!("chart relationship {id} of {}", slide.part))
                    })?;
                let data = self
                    .package
                    .part(&part)
                    .ok_or_else(|| Error::MissingPart(part.to_string()))?;
                read_chart(data.data())
            })
            .collect()
    }
}
