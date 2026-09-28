//! Construction of the typed `c:chartSpace` for a [`Chart`].

use openxml_schema::dml::{
    self, CT_RegularTextRun, CT_SRgbColor, CT_ShapeProperties, CT_SolidColorFillProperties, CT_TextBody,
    CT_TextBodyProperties, CT_TextCharacterProperties, CT_TextListStyle, CT_TextParagraph,
    CT_TextParagraphProperties, EG_ColorChoice, EG_FillProperties, EG_LineFillProperties, EG_TextRun,
};
use openxml_schema::dml_chart::*;
use openxml_xml::HexBinary;

use crate::spec::{Chart, ChartKind, Grouping, LegendPosition, Series};

/// Name of the worksheet that holds the data of an embedded workbook.
pub const DATA_SHEET: &str = "Sheet1";

const CAT_AX: u32 = 500_000_001;
const VAL_AX: u32 = 500_000_002;

/// Where series data comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource<'a> {
    /// Values are stored in the chart only (`c:strLit` / `c:numLit`), unless a
    /// series carries explicit spreadsheet references.
    Literal,
    /// Values reference `Sheet1` of an embedded workbook related to the chart
    /// with the given relationship id (`c:externalData`).
    Embedded(&'a str),
}

/// Column name for a 1-based column number (1 → `A`).
pub fn column_name(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.push(b'A' + r as u8);
        n = (n - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).expect("ASCII")
}

fn boolean(v: bool) -> Box<CT_Boolean> {
    Box::new(CT_Boolean {
        val: Some(v),
        ..Default::default()
    })
}

fn uint(v: u32) -> Box<CT_UnsignedInt> {
    Box::new(CT_UnsignedInt {
        val: Some(v),
        ..Default::default()
    })
}

fn number(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        String::new()
    }
}

fn str_data(values: &[String]) -> CT_StrData {
    CT_StrData {
        pt_count: Some(uint(values.len() as u32)),
        pt: values
            .iter()
            .enumerate()
            .map(|(i, v)| CT_StrVal {
                idx: Some(i as u32),
                v: Some(v.clone()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn num_data(values: &[f64], format: &str) -> CT_NumData {
    CT_NumData {
        format_code: Some(format.to_owned()),
        pt_count: Some(uint(values.len() as u32)),
        // Missing points (NaN) are omitted, which charts show as gaps.
        pt: values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .map(|(i, v)| CT_NumVal {
                idx: Some(i as u32),
                v: Some(number(*v)),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn solid_fill(rgb: [u8; 3]) -> CT_SolidColorFillProperties {
    CT_SolidColorFillProperties {
        color_choice: Some(EG_ColorChoice::SrgbClr(Box::new(CT_SRgbColor {
            val: Some(HexBinary(rgb.to_vec())),
            ..Default::default()
        }))),
        ..Default::default()
    }
}

/// A text body with one paragraph holding `text` (optionally bold at `size_pt`).
fn rich_text(text: &str, size_hundredths: Option<i32>, bold: bool) -> CT_TextBody {
    let r_pr = CT_TextCharacterProperties {
        sz: size_hundredths,
        b: bold.then_some(true),
        ..Default::default()
    };
    CT_TextBody {
        body_pr: Some(Box::new(CT_TextBodyProperties::default())),
        lst_style: Some(Box::new(CT_TextListStyle::default())),
        p: vec![CT_TextParagraph {
            p_pr: Some(Box::new(CT_TextParagraphProperties {
                def_r_pr: Some(Box::new(r_pr.clone())),
                ..Default::default()
            })),
            text_run: vec![EG_TextRun::R(Box::new(CT_RegularTextRun {
                r_pr: Some(Box::new(CT_TextCharacterProperties {
                    lang: Some("en-US".into()),
                    ..r_pr
                })),
                t: Some(text.to_owned()),
                ..Default::default()
            }))],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn title(text: &str, size_hundredths: i32) -> Box<CT_Title> {
    Box::new(CT_Title {
        tx: Some(Box::new(CT_Tx {
            choice: Some(CT_Tx_Choice::Rich(Box::new(rich_text(
                text,
                Some(size_hundredths),
                true,
            )))),
            ..Default::default()
        })),
        overlay: Some(boolean(false)),
        ..Default::default()
    })
}

struct Refs {
    name: Option<String>,
    values: Option<String>,
    x_values: Option<String>,
}

/// Spreadsheet references of series `i` (explicit ones win over the embedded layout).
fn series_refs(chart: &Chart, i: usize, source: DataSource<'_>) -> Refs {
    let s = &chart.series[i];
    let rows = chart.point_count().max(1) as u32;
    let (x_col, y_col) = if chart.kind == ChartKind::Scatter {
        (2 * i as u32 + 1, 2 * i as u32 + 2)
    } else {
        (1, i as u32 + 2)
    };
    let embedded = matches!(source, DataSource::Embedded(_));
    let range = |col: u32| {
        let c = column_name(col);
        format!("{DATA_SHEET}!${c}$2:${c}${}", rows + 1)
    };
    Refs {
        name: s
            .name_ref
            .clone()
            .or_else(|| embedded.then(|| format!("{DATA_SHEET}!${}$1", column_name(y_col)))),
        values: s.values_ref.clone().or_else(|| embedded.then(|| range(y_col))),
        x_values: s
            .x_values_ref
            .clone()
            .or_else(|| (embedded && chart.kind == ChartKind::Scatter).then(|| range(x_col))),
    }
}

fn categories_ref(chart: &Chart, source: DataSource<'_>) -> Option<String> {
    chart.categories_ref.clone().or_else(|| {
        matches!(source, DataSource::Embedded(_))
            .then(|| format!("{DATA_SHEET}!$A$2:$A${}", chart.point_count().max(1) + 1))
    })
}

/// The x values of a scatter series (1, 2, 3, … when none are given).
pub(crate) fn scatter_x(s: &Series) -> Vec<f64> {
    if s.x_values.is_empty() {
        (1..=s.values.len()).map(|i| i as f64).collect()
    } else {
        s.x_values.clone()
    }
}

struct SeriesParts {
    idx: Box<CT_UnsignedInt>,
    tx: Box<CT_SerTx>,
    cat: Option<Box<CT_AxDataSource>>,
    val: Box<CT_NumDataSource>,
    x_val: Option<Box<CT_AxDataSource>>,
    sp_pr: Option<Box<CT_ShapeProperties>>,
}

fn series_parts(chart: &Chart, i: usize, source: DataSource<'_>) -> SeriesParts {
    let s = &chart.series[i];
    let refs = series_refs(chart, i, source);
    let tx = match refs.name {
        Some(f) => CT_SerTx_Choice::StrRef(Box::new(CT_StrRef {
            f: Some(f),
            str_cache: Some(Box::new(str_data(std::slice::from_ref(&s.name)))),
            ..Default::default()
        })),
        None => CT_SerTx_Choice::V(s.name.clone()),
    };
    let values = match refs.values {
        Some(f) => CT_NumDataSource_Choice::NumRef(Box::new(CT_NumRef {
            f: Some(f),
            num_cache: Some(Box::new(num_data(&s.values, &chart.number_format))),
            ..Default::default()
        })),
        None => CT_NumDataSource_Choice::NumLit(Box::new(num_data(&s.values, &chart.number_format))),
    };
    let cat = (chart.kind != ChartKind::Scatter && !chart.categories.is_empty()).then(|| {
        Box::new(CT_AxDataSource {
            choice: Some(match categories_ref(chart, source) {
                Some(f) => CT_AxDataSource_Choice::StrRef(Box::new(CT_StrRef {
                    f: Some(f),
                    str_cache: Some(Box::new(str_data(&chart.categories))),
                    ..Default::default()
                })),
                None => CT_AxDataSource_Choice::StrLit(Box::new(str_data(&chart.categories))),
            }),
            ..Default::default()
        })
    });
    let x_val = (chart.kind == ChartKind::Scatter).then(|| {
        let xs = scatter_x(s);
        Box::new(CT_AxDataSource {
            choice: Some(match refs.x_values {
                Some(f) => CT_AxDataSource_Choice::NumRef(Box::new(CT_NumRef {
                    f: Some(f),
                    num_cache: Some(Box::new(num_data(&xs, "General"))),
                    ..Default::default()
                })),
                None => CT_AxDataSource_Choice::NumLit(Box::new(num_data(&xs, "General"))),
            }),
            ..Default::default()
        })
    });
    let line_like = matches!(chart.kind, ChartKind::Line | ChartKind::Scatter)
        || (chart.kind == ChartKind::Radar && !chart.radar_filled);
    let sp_pr = s.color.map(|rgb| {
        let mut props = CT_ShapeProperties::default();
        if line_like {
            props.ln = Some(Box::new(dml::CT_LineProperties {
                w: Some(28_575),
                line_fill_properties: Some(EG_LineFillProperties::SolidFill(Box::new(solid_fill(rgb)))),
                ..Default::default()
            }));
        } else {
            props.fill_properties = Some(EG_FillProperties::SolidFill(Box::new(solid_fill(rgb))));
        }
        Box::new(props)
    });
    let sp_pr = if chart.kind == ChartKind::Scatter && !chart.scatter_lines {
        // Markers only: hide the connecting line.
        let mut props = sp_pr.map(|b| *b).unwrap_or_default();
        props.ln = Some(Box::new(dml::CT_LineProperties {
            w: Some(19_050),
            line_fill_properties: Some(EG_LineFillProperties::NoFill(Box::default())),
            ..Default::default()
        }));
        Some(Box::new(props))
    } else {
        sp_pr
    };
    SeriesParts {
        idx: uint(i as u32),
        tx: Box::new(CT_SerTx {
            choice: Some(tx),
            ..Default::default()
        }),
        cat,
        val: Box::new(CT_NumDataSource {
            choice: Some(values),
            ..Default::default()
        }),
        x_val,
        sp_pr,
    }
}

fn data_labels(chart: &Chart) -> Option<Box<CT_DLbls>> {
    if !chart.data_labels {
        return None;
    }
    let pie = matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut);
    Some(Box::new(CT_DLbls {
        choice: vec![
            CT_DLbls_Choice::ShowLegendKey(boolean(false)),
            CT_DLbls_Choice::ShowVal(boolean(true)),
            CT_DLbls_Choice::ShowCatName(boolean(false)),
            CT_DLbls_Choice::ShowSerName(boolean(false)),
            CT_DLbls_Choice::ShowPercent(boolean(false)),
            CT_DLbls_Choice::ShowBubbleSize(boolean(false)),
            CT_DLbls_Choice::ShowLeaderLines(boolean(pie)),
        ],
        ..Default::default()
    }))
}

fn marker(show: bool) -> Option<Box<CT_Marker>> {
    (!show).then(|| {
        Box::new(CT_Marker {
            symbol: Some(Box::new(CT_MarkerStyle {
                val: Some(ST_MarkerStyle::None),
                ..Default::default()
            })),
            ..Default::default()
        })
    })
}

fn axis_ids() -> Vec<CT_UnsignedInt> {
    vec![*uint(CAT_AX), *uint(VAL_AX)]
}

fn plot_chart(chart: &Chart, source: DataSource<'_>) -> CT_PlotArea_Choice {
    let parts = (0..chart.series.len()).map(|i| series_parts(chart, i, source));
    let bar_grouping = match chart.grouping {
        Grouping::Standard => ST_BarGrouping::Clustered,
        Grouping::Stacked => ST_BarGrouping::Stacked,
        Grouping::PercentStacked => ST_BarGrouping::PercentStacked,
    };
    let grouping = match chart.grouping {
        Grouping::Standard => ST_Grouping::Standard,
        Grouping::Stacked => ST_Grouping::Stacked,
        Grouping::PercentStacked => ST_Grouping::PercentStacked,
    };
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => CT_PlotArea_Choice::BarChart(Box::new(CT_BarChart {
            bar_dir: Some(Box::new(CT_BarDir {
                val: Some(if chart.kind == ChartKind::Bar {
                    ST_BarDir::Bar
                } else {
                    ST_BarDir::Col
                }),
                ..Default::default()
            })),
            grouping: Some(Box::new(CT_BarGrouping {
                val: Some(bar_grouping),
                ..Default::default()
            })),
            vary_colors: Some(boolean(false)),
            ser: parts
                .map(|p| CT_BarSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    invert_if_negative: Some(boolean(false)),
                    cat: p.cat,
                    val: Some(p.val),
                    ..Default::default()
                })
                .collect(),
            d_lbls: data_labels(chart),
            gap_width: Some(Box::new(CT_GapAmount {
                val: Some(ST_GapAmount::GapAmountUShort(150)),
                ..Default::default()
            })),
            overlap: (chart.grouping != Grouping::Standard).then(|| {
                Box::new(CT_Overlap {
                    val: Some(ST_Overlap::OverlapByte(100)),
                    ..Default::default()
                })
            }),
            ax_id: axis_ids(),
            ..Default::default()
        })),
        ChartKind::Line => CT_PlotArea_Choice::LineChart(Box::new(CT_LineChart {
            grouping: Some(Box::new(CT_Grouping {
                val: Some(grouping),
                ..Default::default()
            })),
            vary_colors: Some(boolean(false)),
            ser: parts
                .map(|p| CT_LineSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    marker: marker(chart.markers),
                    cat: p.cat,
                    val: Some(p.val),
                    smooth: Some(boolean(chart.smooth)),
                    ..Default::default()
                })
                .collect(),
            d_lbls: data_labels(chart),
            marker: Some(boolean(true)),
            ax_id: axis_ids(),
            ..Default::default()
        })),
        ChartKind::Area => CT_PlotArea_Choice::AreaChart(Box::new(CT_AreaChart {
            grouping: Some(Box::new(CT_Grouping {
                val: Some(grouping),
                ..Default::default()
            })),
            vary_colors: Some(boolean(false)),
            ser: parts
                .map(|p| CT_AreaSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    cat: p.cat,
                    val: Some(p.val),
                    ..Default::default()
                })
                .collect(),
            d_lbls: data_labels(chart),
            ax_id: axis_ids(),
            ..Default::default()
        })),
        ChartKind::Pie | ChartKind::Doughnut => {
            let ser = parts
                .map(|p| CT_PieSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    cat: p.cat,
                    val: Some(p.val),
                    ..Default::default()
                })
                .collect();
            let first_slice = Some(Box::new(CT_FirstSliceAng {
                val: Some(0),
                ..Default::default()
            }));
            if chart.kind == ChartKind::Pie {
                CT_PlotArea_Choice::PieChart(Box::new(CT_PieChart {
                    vary_colors: Some(boolean(true)),
                    ser,
                    d_lbls: data_labels(chart),
                    first_slice_ang: first_slice,
                    ..Default::default()
                }))
            } else {
                CT_PlotArea_Choice::DoughnutChart(Box::new(CT_DoughnutChart {
                    vary_colors: Some(boolean(true)),
                    ser,
                    d_lbls: data_labels(chart),
                    first_slice_ang: first_slice,
                    hole_size: Some(Box::new(CT_HoleSize {
                        val: Some(ST_HoleSize::HoleSizeUByte(chart.hole_size)),
                        ..Default::default()
                    })),
                    ..Default::default()
                }))
            }
        }
        ChartKind::Scatter => CT_PlotArea_Choice::ScatterChart(Box::new(CT_ScatterChart {
            scatter_style: Some(Box::new(CT_ScatterStyle {
                val: Some(if chart.smooth {
                    ST_ScatterStyle::SmoothMarker
                } else {
                    ST_ScatterStyle::LineMarker
                }),
                ..Default::default()
            })),
            vary_colors: Some(boolean(false)),
            ser: parts
                .map(|p| CT_ScatterSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    marker: marker(chart.markers),
                    x_val: p.x_val,
                    y_val: Some(p.val),
                    smooth: Some(boolean(chart.smooth)),
                    ..Default::default()
                })
                .collect(),
            d_lbls: data_labels(chart),
            ax_id: axis_ids(),
            ..Default::default()
        })),
        ChartKind::Radar => CT_PlotArea_Choice::RadarChart(Box::new(CT_RadarChart {
            radar_style: Some(Box::new(CT_RadarStyle {
                val: Some(if chart.radar_filled {
                    ST_RadarStyle::Filled
                } else if chart.markers {
                    ST_RadarStyle::Marker
                } else {
                    ST_RadarStyle::Standard
                }),
                ..Default::default()
            })),
            vary_colors: Some(boolean(false)),
            ser: parts
                .map(|p| CT_RadarSer {
                    idx: Some(p.idx.clone()),
                    order: Some(p.idx),
                    tx: Some(p.tx),
                    sp_pr: p.sp_pr,
                    marker: marker(chart.markers || chart.radar_filled),
                    cat: p.cat,
                    val: Some(p.val),
                    ..Default::default()
                })
                .collect(),
            d_lbls: data_labels(chart),
            ax_id: axis_ids(),
            ..Default::default()
        })),
    }
}

fn scaling() -> Option<Box<CT_Scaling>> {
    Some(Box::new(CT_Scaling {
        orientation: Some(Box::new(CT_Orientation {
            val: Some(ST_Orientation::MinMax),
            ..Default::default()
        })),
        ..Default::default()
    }))
}

fn ax_pos(p: ST_AxPos) -> Option<Box<CT_AxPos>> {
    Some(Box::new(CT_AxPos {
        val: Some(p),
        ..Default::default()
    }))
}

fn crosses() -> Option<Box<CT_Crosses>> {
    Some(Box::new(CT_Crosses {
        val: Some(ST_Crosses::AutoZero),
        ..Default::default()
    }))
}

fn tick_labels() -> Option<Box<CT_TickLblPos>> {
    Some(Box::new(CT_TickLblPos {
        val: Some(ST_TickLblPos::NextTo),
        ..Default::default()
    }))
}

fn value_axis(
    id: u32,
    cross: u32,
    pos: ST_AxPos,
    title_text: Option<&str>,
    chart: &Chart,
    gridlines: bool,
) -> CT_ValAx {
    CT_ValAx {
        ax_id: Some(uint(id)),
        scaling: scaling(),
        delete: Some(boolean(false)),
        ax_pos: ax_pos(pos),
        major_gridlines: gridlines.then(|| Box::new(CT_ChartLines::default())),
        title: title_text.map(|t| title(t, 1000)),
        num_fmt: Some(Box::new(CT_NumFmt {
            format_code: Some(chart.number_format.clone()),
            source_linked: Some(true),
            ..Default::default()
        })),
        major_tick_mark: Some(Box::new(CT_TickMark {
            val: Some(ST_TickMark::Out),
            ..Default::default()
        })),
        minor_tick_mark: Some(Box::new(CT_TickMark {
            val: Some(ST_TickMark::None),
            ..Default::default()
        })),
        tick_lbl_pos: tick_labels(),
        cross_ax: Some(uint(cross)),
        choice: Some(CT_ValAx_Choice::Crosses(crosses().expect("set"))),
        cross_between: Some(Box::new(CT_CrossBetween {
            val: Some(if chart.kind == ChartKind::Scatter {
                ST_CrossBetween::MidCat
            } else {
                ST_CrossBetween::Between
            }),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn axes(chart: &Chart) -> Vec<CT_PlotArea_Choice2> {
    if !chart.kind.has_axes() {
        return Vec::new();
    }
    let horizontal = chart.kind == ChartKind::Bar;
    let (cat_pos, val_pos) = if horizontal {
        (ST_AxPos::L, ST_AxPos::B)
    } else {
        (ST_AxPos::B, ST_AxPos::L)
    };
    let x_title = chart.x_axis_title.as_deref();
    let y_title = chart.y_axis_title.as_deref();
    if chart.kind == ChartKind::Scatter {
        return vec![
            CT_PlotArea_Choice2::ValAx(Box::new(value_axis(
                CAT_AX,
                VAL_AX,
                ST_AxPos::B,
                x_title,
                chart,
                false,
            ))),
            CT_PlotArea_Choice2::ValAx(Box::new(value_axis(
                VAL_AX,
                CAT_AX,
                ST_AxPos::L,
                y_title,
                chart,
                true,
            ))),
        ];
    }
    let cat = CT_CatAx {
        ax_id: Some(uint(CAT_AX)),
        scaling: scaling(),
        delete: Some(boolean(false)),
        ax_pos: ax_pos(cat_pos),
        title: x_title.map(|t| title(t, 1000)),
        num_fmt: Some(Box::new(CT_NumFmt {
            format_code: Some("General".into()),
            source_linked: Some(true),
            ..Default::default()
        })),
        major_tick_mark: Some(Box::new(CT_TickMark {
            val: Some(ST_TickMark::Out),
            ..Default::default()
        })),
        minor_tick_mark: Some(Box::new(CT_TickMark {
            val: Some(ST_TickMark::None),
            ..Default::default()
        })),
        tick_lbl_pos: tick_labels(),
        cross_ax: Some(uint(VAL_AX)),
        choice: Some(CT_CatAx_Choice::Crosses(crosses().expect("set"))),
        auto: Some(boolean(true)),
        lbl_algn: Some(Box::new(CT_LblAlgn {
            val: Some(ST_LblAlgn::Ctr),
            ..Default::default()
        })),
        lbl_offset: Some(Box::new(CT_LblOffset {
            val: Some(ST_LblOffset::LblOffsetUShort(100)),
            ..Default::default()
        })),
        no_multi_lvl_lbl: Some(boolean(false)),
        ..Default::default()
    };
    vec![
        CT_PlotArea_Choice2::CatAx(Box::new(cat)),
        CT_PlotArea_Choice2::ValAx(Box::new(value_axis(
            VAL_AX, CAT_AX, val_pos, y_title, chart, true,
        ))),
    ]
}

impl Chart {
    /// Builds the typed chart part.
    pub fn to_chart_space(&self, source: DataSource<'_>) -> CT_ChartSpace {
        let legend = self.legend.map(|pos| {
            Box::new(CT_Legend {
                legend_pos: Some(Box::new(CT_LegendPos {
                    val: Some(match pos {
                        LegendPosition::Right => ST_LegendPos::R,
                        LegendPosition::Left => ST_LegendPos::L,
                        LegendPosition::Top => ST_LegendPos::T,
                        LegendPosition::Bottom => ST_LegendPos::B,
                        LegendPosition::TopRight => ST_LegendPos::Tr,
                    }),
                    ..Default::default()
                })),
                overlay: Some(boolean(false)),
                ..Default::default()
            })
        });
        let chart = CT_Chart {
            title: self.title.as_deref().map(|t| title(t, 1400)),
            auto_title_deleted: Some(boolean(self.title.is_none())),
            plot_area: Some(Box::new(CT_PlotArea {
                layout: Some(Box::default()),
                choice: vec![plot_chart(self, source)],
                choice_2: axes(self),
                ..Default::default()
            })),
            legend,
            plot_vis_only: Some(boolean(true)),
            disp_blanks_as: Some(Box::new(CT_DispBlanksAs {
                val: Some(ST_DispBlanksAs::Gap),
                ..Default::default()
            })),
            ..Default::default()
        };
        CT_ChartSpace {
            date1904: Some(boolean(false)),
            rounded_corners: Some(boolean(false)),
            chart: Some(Box::new(chart)),
            external_data: match source {
                DataSource::Embedded(id) => Some(Box::new(CT_ExternalData {
                    r_id: Some(id.to_owned()),
                    auto_update: Some(boolean(false)),
                    ..Default::default()
                })),
                DataSource::Literal => None,
            },
            ..Default::default()
        }
    }

    /// Serializes the chart part with literal data (no embedded workbook).
    pub fn to_xml(&self) -> String {
        elements::CHART_SPACE.to_xml(&self.to_chart_space(DataSource::Literal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names() {
        assert_eq!(column_name(1), "A");
        assert_eq!(column_name(26), "Z");
        assert_eq!(column_name(27), "AA");
        assert_eq!(column_name(702), "ZZ");
        assert_eq!(column_name(703), "AAA");
    }

    #[test]
    fn embedded_references_follow_the_sheet_layout() {
        let c = Chart::new(ChartKind::Line)
            .categories(["a", "b", "c"])
            .series(Series::new("one", [1.0, 2.0, 3.0]))
            .series(Series::new("two", [4.0, 5.0, 6.0]));
        let refs = series_refs(&c, 1, DataSource::Embedded("rId1"));
        assert_eq!(refs.name.as_deref(), Some("Sheet1!$C$1"));
        assert_eq!(refs.values.as_deref(), Some("Sheet1!$C$2:$C$4"));
        assert_eq!(
            categories_ref(&c, DataSource::Embedded("rId1")).as_deref(),
            Some("Sheet1!$A$2:$A$4")
        );
        assert!(series_refs(&c, 0, DataSource::Literal).values.is_none());
        let s = Chart::new(ChartKind::Scatter)
            .series(Series::new("p", [1.0, 2.0]))
            .series(Series::new("q", [3.0]));
        let r = series_refs(&s, 1, DataSource::Embedded("rId1"));
        assert_eq!(r.x_values.as_deref(), Some("Sheet1!$C$2:$C$3"));
        assert_eq!(r.values.as_deref(), Some("Sheet1!$D$2:$D$3"));
    }

    #[test]
    fn missing_points_are_omitted_and_numbers_are_plain() {
        let d = num_data(&[1.5, f64::NAN, 3.0], "General");
        assert_eq!(d.pt.len(), 2);
        assert_eq!(d.pt[1].idx, Some(2));
        assert_eq!(d.pt[1].v.as_deref(), Some("3"));
        assert_eq!(d.pt_count.as_ref().unwrap().val, Some(3));
        assert_eq!(scatter_x(&Series::new("s", [5.0, 6.0])), [1.0, 2.0]);
    }
}
