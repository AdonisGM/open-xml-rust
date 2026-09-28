//! DrawingML charts (ECMA-376 Part 1 §21.2) for Word, Excel and PowerPoint.
//!
//! A [`Chart`] describes a chart independently of the host format: kind,
//! categories, series, title, legend, axis titles and labels. It is turned
//! into the generated `c:chartSpace` type ([`Chart::to_chart_space`]) and
//! added to a package with [`insert_chart`], optionally together with an
//! embedded workbook holding the data (what Office's "Edit Data" opens). The
//! host format then shows the chart by wrapping [`graphic_data`] in its own
//! drawing element. [`read_chart`] reads the data back from any chart part.
//!
//! ```
//! use openxml_chart::{Chart, ChartKind, Series, read_chart};
//!
//! let chart = Chart::new(ChartKind::Pie)
//!     .title("Market share")
//!     .categories(["North", "South", "East"])
//!     .series(Series::new("2024", [45.0, 30.0, 25.0]))
//!     .data_labels(true);
//! let info = read_chart(chart.to_xml().as_bytes())?;
//! assert_eq!(info.title.as_deref(), Some("Market share"));
//! assert_eq!(info.plots[0].series[0].values, [Some(45.0), Some(30.0), Some(25.0)]);
//! # Ok::<(), openxml_core::Error>(())
//! ```

#![warn(missing_docs)]

mod build;
mod package;
mod read;
mod spec;
mod workbook;

pub use build::{DATA_SHEET, DataSource, column_name};
pub use package::{
    CHART_URI, ChartData, InsertedChart, XLSX_CONTENT_TYPE, chart_reference, chart_rel_id, graphic_data,
    insert_chart,
};
pub use read::{ChartInfo, PlotInfo, SeriesInfo, chart_info, read_chart};
pub use spec::{Chart, ChartKind, Grouping, LegendPosition, Series};
pub use workbook::{data_sheet, embedded_workbook};
