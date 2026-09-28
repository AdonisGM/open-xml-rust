//! A format-independent description of a chart.

/// Kind of chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartKind {
    /// Vertical bars (`c:barChart` with `barDir="col"`).
    Column,
    /// Horizontal bars (`c:barChart` with `barDir="bar"`).
    Bar,
    /// Lines (`c:lineChart`).
    Line,
    /// Filled areas (`c:areaChart`).
    Area,
    /// Pie (`c:pieChart`).
    Pie,
    /// Doughnut (`c:doughnutChart`).
    Doughnut,
    /// XY scatter (`c:scatterChart`).
    Scatter,
    /// Radar (`c:radarChart`).
    Radar,
}

impl ChartKind {
    /// Whether the chart plots series against category and value axes.
    pub fn has_axes(self) -> bool {
        !matches!(self, ChartKind::Pie | ChartKind::Doughnut)
    }
}

/// How the series of bar, column, line and area charts are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Grouping {
    /// Side by side (bars) or independent (lines, areas).
    #[default]
    Standard,
    /// Stacked on top of each other.
    Stacked,
    /// Stacked and scaled to 100 %.
    PercentStacked,
}

/// Where the legend is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LegendPosition {
    /// Right of the plot area.
    #[default]
    Right,
    /// Left of the plot area.
    Left,
    /// Above the plot area.
    Top,
    /// Below the plot area.
    Bottom,
    /// Top-right corner.
    TopRight,
}

/// A data series.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    /// Series name (legend entry).
    pub name: String,
    /// Values (y values for scatter charts). `NaN` marks a missing point.
    pub values: Vec<f64>,
    /// X values of a scatter series (defaults to 1, 2, 3, …).
    pub x_values: Vec<f64>,
    /// Fill (or line) colour as RGB.
    pub color: Option<[u8; 3]>,
    /// Cell reference of the name, e.g. `Sheet1!$B$1` (spreadsheets).
    pub name_ref: Option<String>,
    /// Cell range of the values, e.g. `Sheet1!$B$2:$B$5` (spreadsheets).
    pub values_ref: Option<String>,
    /// Cell range of the x values of a scatter series.
    pub x_values_ref: Option<String>,
}

impl Series {
    /// Creates a series.
    pub fn new(name: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Self {
        Series {
            name: name.into(),
            values: values.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Sets the x values (scatter charts).
    pub fn x_values(mut self, xs: impl IntoIterator<Item = f64>) -> Self {
        self.x_values = xs.into_iter().collect();
        self
    }

    /// Sets the colour from a `0xRRGGBB` value.
    pub fn color(mut self, rgb: u32) -> Self {
        self.color = Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
        self
    }

    /// Takes the name, values and x values from spreadsheet ranges (used in
    /// workbooks, where the chart reads its data from the sheets).
    pub fn references(mut self, name: Option<&str>, values: &str, x_values: Option<&str>) -> Self {
        self.name_ref = name.map(str::to_owned);
        self.values_ref = Some(values.to_owned());
        self.x_values_ref = x_values.map(str::to_owned);
        self
    }
}

/// A chart: kind, data and presentation options.
///
/// ```
/// use openxml_chart::{Chart, ChartKind, LegendPosition, Series};
///
/// let chart = Chart::new(ChartKind::Column)
///     .title("Revenue")
///     .categories(["Q1", "Q2", "Q3", "Q4"])
///     .series(Series::new("2023", [10.0, 12.0, 9.5, 14.0]).color(0x4472C4))
///     .series(Series::new("2024", [11.0, 13.5, 12.0, 16.0]).color(0xED7D31))
///     .legend(Some(LegendPosition::Bottom))
///     .axis_titles("Quarter", "Million USD");
/// let xml = chart.to_xml();
/// assert!(xml.contains("<c:barChart>"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    /// Chart kind.
    pub kind: ChartKind,
    /// Series grouping (bar, column, line, area).
    pub grouping: Grouping,
    /// Title shown above the chart.
    pub title: Option<String>,
    /// Category labels (not used by scatter charts).
    pub categories: Vec<String>,
    /// Cell range of the categories (spreadsheets).
    pub categories_ref: Option<String>,
    /// Data series.
    pub series: Vec<Series>,
    /// Legend position; `None` hides the legend.
    pub legend: Option<LegendPosition>,
    /// Show values as data labels.
    pub data_labels: bool,
    /// Title of the category (x) axis.
    pub x_axis_title: Option<String>,
    /// Title of the value (y) axis.
    pub y_axis_title: Option<String>,
    /// Draw markers on line, scatter and radar series.
    pub markers: bool,
    /// Smooth lines (line and scatter charts).
    pub smooth: bool,
    /// Draw lines between scatter points.
    pub scatter_lines: bool,
    /// Fill radar series.
    pub radar_filled: bool,
    /// Hole size of a doughnut chart in percent (10–90).
    pub hole_size: u8,
    /// Number format of the values, e.g. `#,##0.00`.
    pub number_format: String,
}

impl Chart {
    /// Creates an empty chart of the given kind.
    pub fn new(kind: ChartKind) -> Self {
        Chart {
            kind,
            grouping: Grouping::Standard,
            title: None,
            categories: Vec::new(),
            categories_ref: None,
            series: Vec::new(),
            legend: Some(LegendPosition::Right),
            data_labels: false,
            x_axis_title: None,
            y_axis_title: None,
            markers: matches!(kind, ChartKind::Line | ChartKind::Scatter | ChartKind::Radar),
            smooth: false,
            scatter_lines: false,
            radar_filled: false,
            hole_size: 50,
            number_format: "General".into(),
        }
    }

    /// Sets the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the category labels.
    pub fn categories<S: Into<String>>(mut self, labels: impl IntoIterator<Item = S>) -> Self {
        self.categories = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Takes the categories from a spreadsheet range (the labels still fill the cache).
    pub fn categories_ref(mut self, range: impl Into<String>) -> Self {
        self.categories_ref = Some(range.into());
        self
    }

    /// Adds a series.
    pub fn series(mut self, series: Series) -> Self {
        self.series.push(series);
        self
    }

    /// Sets the grouping.
    pub fn grouping(mut self, grouping: Grouping) -> Self {
        self.grouping = grouping;
        self
    }

    /// Sets (or hides, with `None`) the legend.
    pub fn legend(mut self, position: Option<LegendPosition>) -> Self {
        self.legend = position;
        self
    }

    /// Shows values as data labels.
    pub fn data_labels(mut self, show: bool) -> Self {
        self.data_labels = show;
        self
    }

    /// Sets the axis titles.
    pub fn axis_titles(mut self, x: impl Into<String>, y: impl Into<String>) -> Self {
        self.x_axis_title = Some(x.into());
        self.y_axis_title = Some(y.into());
        self
    }

    /// Shows or hides markers.
    pub fn markers(mut self, show: bool) -> Self {
        self.markers = show;
        self
    }

    /// Smooths lines.
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Connects scatter points with lines.
    pub fn scatter_lines(mut self, lines: bool) -> Self {
        self.scatter_lines = lines;
        self
    }

    /// Fills radar series.
    pub fn radar_filled(mut self, filled: bool) -> Self {
        self.radar_filled = filled;
        self
    }

    /// Sets the doughnut hole size (clamped to 10–90 %).
    pub fn hole_size(mut self, percent: u8) -> Self {
        self.hole_size = percent.clamp(10, 90);
        self
    }

    /// Sets the number format of the values.
    pub fn number_format(mut self, code: impl Into<String>) -> Self {
        self.number_format = code.into();
        self
    }

    /// Number of data points (the longest series or the categories).
    pub fn point_count(&self) -> usize {
        self.series
            .iter()
            .map(|s| s.values.len())
            .chain([self.categories.len()])
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_every_option() {
        let c = Chart::new(ChartKind::Doughnut)
            .title("T")
            .categories(["a", "b"])
            .categories_ref("Sheet1!$A$2:$A$3")
            .series(Series::new("s", [1.0, 2.0, 3.0]).color(0x112233).x_values([1.0]))
            .grouping(Grouping::Stacked)
            .legend(None)
            .data_labels(true)
            .axis_titles("x", "y")
            .markers(false)
            .smooth(true)
            .scatter_lines(true)
            .radar_filled(true)
            .hole_size(99)
            .number_format("0%");
        assert_eq!(c.title.as_deref(), Some("T"));
        assert_eq!(c.categories, ["a", "b"]);
        assert_eq!(c.series[0].color, Some([0x11, 0x22, 0x33]));
        assert_eq!(c.hole_size, 90);
        assert_eq!(c.point_count(), 3);
        assert!(c.legend.is_none() && c.data_labels && c.smooth && c.scatter_lines && c.radar_filled);
        assert_eq!(c.number_format, "0%");
        assert!(!ChartKind::Pie.has_axes() && ChartKind::Scatter.has_axes());
        assert!(Chart::new(ChartKind::Line).markers);
        assert!(!Chart::new(ChartKind::Column).markers);
        let s = Series::new("n", [1.0]).references(Some("S!$B$1"), "S!$B$2", None);
        assert_eq!(s.values_ref.as_deref(), Some("S!$B$2"));
    }
}
