//! Reading the data of existing charts (from the caches stored in the chart part).

use openxml_core::{Error, Result};
use openxml_schema::dml;
use openxml_schema::dml_chart::*;

use crate::spec::ChartKind;

/// A series read from a chart part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeriesInfo {
    /// Series name.
    pub name: Option<String>,
    /// Category labels (or x values of scatter charts, formatted).
    pub categories: Vec<String>,
    /// Values; `None` for missing points.
    pub values: Vec<Option<f64>>,
    /// Spreadsheet reference of the values, if any.
    pub values_ref: Option<String>,
}

/// One plot (chart type) of a chart part.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotInfo {
    /// Chart kind, or `None` for kinds without a [`ChartKind`] (3-D, stock, surface, …).
    pub kind: Option<ChartKind>,
    /// Schema element name, e.g. `barChart`.
    pub element: String,
    /// Series in order.
    pub series: Vec<SeriesInfo>,
}

/// What a chart part shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartInfo {
    /// Title text.
    pub title: Option<String>,
    /// Plots (a combination chart has several).
    pub plots: Vec<PlotInfo>,
}

fn text_body(body: &dml::CT_TextBody) -> String {
    body.p
        .iter()
        .map(|p| {
            p.text_run
                .iter()
                .filter_map(|r| match r {
                    dml::EG_TextRun::R(r) => r.t.clone(),
                    dml::EG_TextRun::Fld(f) => f.t.clone(),
                    _ => None,
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strings(data: &CT_StrData) -> Vec<String> {
    let n = data.pt_count.as_ref().and_then(|c| c.val).unwrap_or(0) as usize;
    let mut out = vec![String::new(); n.max(data.pt.len())];
    for p in &data.pt {
        if let (Some(i), Some(v)) = (p.idx, &p.v)
            && let Some(slot) = out.get_mut(i as usize)
        {
            *slot = v.clone();
        }
    }
    out
}

fn numbers(data: &CT_NumData) -> Vec<Option<f64>> {
    let n = data.pt_count.as_ref().and_then(|c| c.val).unwrap_or(0) as usize;
    let mut out = vec![None; n.max(data.pt.len())];
    for p in &data.pt {
        if let (Some(i), Some(v)) = (p.idx, &p.v)
            && let Some(slot) = out.get_mut(i as usize)
        {
            *slot = v.trim().parse().ok();
        }
    }
    out
}

fn series_name(tx: &Option<Box<CT_SerTx>>) -> Option<String> {
    match tx.as_ref()?.choice.as_ref()? {
        CT_SerTx_Choice::StrRef(r) => r.str_cache.as_ref().map(|c| strings(c).join(" ")),
        CT_SerTx_Choice::V(v) => Some(v.clone()),
        CT_SerTx_Choice::Other(_) => None,
    }
}

fn categories(cat: &Option<Box<CT_AxDataSource>>) -> Vec<String> {
    let Some(choice) = cat.as_ref().and_then(|c| c.choice.as_ref()) else {
        return Vec::new();
    };
    match choice {
        CT_AxDataSource_Choice::StrRef(r) => r.str_cache.as_ref().map(|c| strings(c)).unwrap_or_default(),
        CT_AxDataSource_Choice::StrLit(d) => strings(d),
        CT_AxDataSource_Choice::NumRef(r) => r
            .num_cache
            .as_ref()
            .map(|c| {
                numbers(c)
                    .into_iter()
                    .map(|v| v.map(|x| x.to_string()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default(),
        CT_AxDataSource_Choice::NumLit(d) => numbers(d)
            .into_iter()
            .map(|v| v.map(|x| x.to_string()).unwrap_or_default())
            .collect(),
        CT_AxDataSource_Choice::MultiLvlStrRef(r) => r
            .multi_lvl_str_cache
            .as_ref()
            .and_then(|c| c.lvl.first())
            .map(|l| {
                let mut out = Vec::new();
                for p in &l.pt {
                    out.push(p.v.clone().unwrap_or_default());
                }
                out
            })
            .unwrap_or_default(),
        CT_AxDataSource_Choice::Other(_) => Vec::new(),
    }
}

fn values(val: &Option<Box<CT_NumDataSource>>) -> (Vec<Option<f64>>, Option<String>) {
    match val.as_ref().and_then(|v| v.choice.as_ref()) {
        Some(CT_NumDataSource_Choice::NumRef(r)) => (
            r.num_cache.as_ref().map(|c| numbers(c)).unwrap_or_default(),
            r.f.clone(),
        ),
        Some(CT_NumDataSource_Choice::NumLit(d)) => (numbers(d), None),
        _ => (Vec::new(), None),
    }
}

macro_rules! series_list {
    ($ser:expr, cat = $cat:ident, val = $val:ident) => {
        $ser.iter()
            .map(|s| {
                let (values, values_ref) = values(&s.$val);
                SeriesInfo {
                    name: series_name(&s.tx),
                    categories: categories(&s.$cat),
                    values,
                    values_ref,
                }
            })
            .collect()
    };
}

fn plot(choice: &CT_PlotArea_Choice) -> Option<PlotInfo> {
    let (kind, element, series): (Option<ChartKind>, &str, Vec<SeriesInfo>) = match choice {
        CT_PlotArea_Choice::BarChart(c) => {
            let horizontal = c.bar_dir.as_ref().and_then(|d| d.val) == Some(ST_BarDir::Bar);
            (
                Some(if horizontal {
                    ChartKind::Bar
                } else {
                    ChartKind::Column
                }),
                "barChart",
                series_list!(c.ser, cat = cat, val = val),
            )
        }
        CT_PlotArea_Choice::Bar3DChart(c) => (None, "bar3DChart", series_list!(c.ser, cat = cat, val = val)),
        CT_PlotArea_Choice::LineChart(c) => (
            Some(ChartKind::Line),
            "lineChart",
            series_list!(c.ser, cat = cat, val = val),
        ),
        CT_PlotArea_Choice::Line3DChart(c) => {
            (None, "line3DChart", series_list!(c.ser, cat = cat, val = val))
        }
        CT_PlotArea_Choice::AreaChart(c) => (
            Some(ChartKind::Area),
            "areaChart",
            series_list!(c.ser, cat = cat, val = val),
        ),
        CT_PlotArea_Choice::Area3DChart(c) => {
            (None, "area3DChart", series_list!(c.ser, cat = cat, val = val))
        }
        CT_PlotArea_Choice::PieChart(c) => (
            Some(ChartKind::Pie),
            "pieChart",
            series_list!(c.ser, cat = cat, val = val),
        ),
        CT_PlotArea_Choice::Pie3DChart(c) => (None, "pie3DChart", series_list!(c.ser, cat = cat, val = val)),
        CT_PlotArea_Choice::OfPieChart(c) => (None, "ofPieChart", series_list!(c.ser, cat = cat, val = val)),
        CT_PlotArea_Choice::DoughnutChart(c) => (
            Some(ChartKind::Doughnut),
            "doughnutChart",
            series_list!(c.ser, cat = cat, val = val),
        ),
        CT_PlotArea_Choice::ScatterChart(c) => (
            Some(ChartKind::Scatter),
            "scatterChart",
            series_list!(c.ser, cat = x_val, val = y_val),
        ),
        CT_PlotArea_Choice::RadarChart(c) => (
            Some(ChartKind::Radar),
            "radarChart",
            series_list!(c.ser, cat = cat, val = val),
        ),
        CT_PlotArea_Choice::StockChart(c) => (None, "stockChart", series_list!(c.ser, cat = cat, val = val)),
        CT_PlotArea_Choice::SurfaceChart(c) => {
            (None, "surfaceChart", series_list!(c.ser, cat = cat, val = val))
        }
        CT_PlotArea_Choice::Surface3DChart(c) => {
            (None, "surface3DChart", series_list!(c.ser, cat = cat, val = val))
        }
        CT_PlotArea_Choice::BubbleChart(c) => {
            (None, "bubbleChart", series_list!(c.ser, cat = x_val, val = y_val))
        }
        CT_PlotArea_Choice::Other(_) => return None,
    };
    Some(PlotInfo {
        kind,
        element: element.to_owned(),
        series,
    })
}

/// Reads a chart part (`c:chartSpace`).
pub fn read_chart(bytes: &[u8]) -> Result<ChartInfo> {
    let space = elements::CHART_SPACE
        .parse_bytes(bytes)
        .map_err(|source| Error::Xml {
            part: "chart".into(),
            source,
        })?;
    Ok(chart_info(&space))
}

/// Describes a typed chart part.
pub fn chart_info(space: &CT_ChartSpace) -> ChartInfo {
    let Some(chart) = space.chart.as_ref() else {
        return ChartInfo {
            title: None,
            plots: Vec::new(),
        };
    };
    let title = chart
        .title
        .as_ref()
        .and_then(|t| t.tx.as_ref())
        .and_then(|tx| match tx.choice.as_ref()? {
            CT_Tx_Choice::Rich(body) => Some(text_body(body)),
            CT_Tx_Choice::StrRef(r) => r.str_cache.as_ref().map(|c| strings(c).join(" ")),
            CT_Tx_Choice::Other(_) => None,
        });
    let plots = chart
        .plot_area
        .as_ref()
        .map(|p| p.choice.iter().filter_map(plot).collect())
        .unwrap_or_default();
    ChartInfo { title, plots }
}
