//! Charts in Word documents (DrawingML charts from `openxml-chart`).

use openxml_chart::{Chart, ChartData, ChartInfo, chart_rel_id, graphic_data, insert_chart, read_chart};
use openxml_core::{Error, Length, Result};
use openxml_schema::wml;

use crate::document::Document;
use crate::drawing::{Floating, frames};
use crate::paragraph::ParagraphMut;
use crate::{text, walk};

impl ParagraphMut<'_> {
    /// Appends a chart, inline or floating. The chart data is stored in an
    /// embedded workbook so that Word's "Edit Data" works. Returns the
    /// drawing object id.
    pub fn add_chart(
        &mut self,
        chart: &Chart,
        width: Length,
        height: Length,
        floating: Option<&Floating>,
    ) -> Result<u32> {
        let part = self.part.clone();
        let inserted = insert_chart(
            &mut self.shared.package,
            &part,
            "/word",
            chart,
            ChartData::EmbeddedWorkbook,
        )?;
        let name = format!("Chart {}", self.shared.next_drawing_id + 1);
        Ok(self.add_graphic(graphic_data(&inserted.rel_id), width, height, &name, floating))
    }
}

impl Document {
    /// Appends a paragraph holding an inline chart.
    ///
    /// ```
    /// use openxml_chart::{Chart, ChartKind, Series};
    /// use openxml_docx::{Document, Length};
    ///
    /// let mut doc = Document::new();
    /// let chart = Chart::new(ChartKind::Line)
    ///     .title("Visitors")
    ///     .categories(["Mon", "Tue", "Wed"])
    ///     .series(Series::new("Site", [120.0, 180.0, 150.0]));
    /// doc.add_chart(&chart, Length::cm(15.0), Length::cm(8.0))?;
    /// let doc = Document::from_bytes(&doc.to_bytes()?)?;
    /// assert_eq!(doc.charts()?[0].title.as_deref(), Some("Visitors"));
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn add_chart(&mut self, chart: &Chart, width: Length, height: Length) -> Result<ParagraphMut<'_>> {
        let mut p = self.add_paragraph("");
        p.add_chart(chart, width, height, None)?;
        Ok(p)
    }

    /// The charts of the document body, in reading order.
    pub fn charts(&self) -> Result<Vec<ChartInfo>> {
        let mut ids = Vec::new();
        walk::walk_blocks_ref(&self.body().block_level_elts, &mut |p: &wml::CT_P| {
            for r in text::runs(&p.p_content) {
                for c in &r.run_inner_content {
                    if let wml::EG_RunInnerContent::Drawing(d) = c {
                        ids.extend(
                            frames(d)
                                .into_iter()
                                .filter_map(|f| f.data.and_then(chart_rel_id)),
                        );
                    }
                }
            }
        });
        let main = &self.shared.main_part;
        ids.iter()
            .map(|id| {
                let part = self
                    .shared
                    .package
                    .relationship_target(Some(main), id)
                    .ok_or_else(|| Error::InvalidDocument(format!("chart relationship {id} of {main}")))?;
                let data = self
                    .shared
                    .package
                    .part(&part)
                    .ok_or_else(|| Error::MissingPart(part.to_string()))?;
                read_chart(data.data())
            })
            .collect()
    }
}
