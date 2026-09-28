//! Well-known relationship types and content types (ECMA-376 Part 1 §11–15, Part 2).
//!
//! Constants use the Transitional spelling. Strict documents use a different
//! relationship-type base URI; [`canonical_relationship_type`] maps them.

use std::borrow::Cow;

/// Base URI of Transitional relationship types.
pub const TRANSITIONAL_REL_BASE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
/// Base URI of Strict relationship types.
pub const STRICT_REL_BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";

/// Maps a Strict relationship type to its Transitional equivalent; other
/// values are returned unchanged.
pub fn canonical_relationship_type(rel_type: &str) -> Cow<'_, str> {
    match rel_type.strip_prefix(STRICT_REL_BASE) {
        Some(rest) => Cow::Owned(format!("{TRANSITIONAL_REL_BASE}{rest}")),
        None => Cow::Borrowed(rel_type),
    }
}

/// Relationship types.
pub mod rel_types {
    macro_rules! rels {
        ($($(#[$m:meta])* $name:ident = $suffix:literal;)*) => {$(
            $(#[$m])*
            pub const $name: &str = concat!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/", $suffix);
        )*};
    }

    /// Core file properties (package-level, Part 2 §8.3).
    pub const CORE_PROPERTIES: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    /// Core properties as written by some older producers.
    pub const CORE_PROPERTIES_LEGACY: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/metadata/core-properties";
    /// Package thumbnail (Part 2).
    pub const THUMBNAIL: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
    /// Digital signature origin (Part 2 §10).
    pub const DIGITAL_SIGNATURE_ORIGIN: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin";
    /// Digital signature (Part 2 §10).
    pub const DIGITAL_SIGNATURE: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature";

    rels! {
        /// Main document part of a package (§11.3.10, §12.3.23, §13.3.6).
        OFFICE_DOCUMENT = "officeDocument";
        /// Extended (application) properties (§15.2.12.3).
        EXTENDED_PROPERTIES = "extended-properties";
        /// Custom properties (§15.2.12.2).
        CUSTOM_PROPERTIES = "custom-properties";
        /// Alternative format import (§11.3.1).
        AF_CHUNK = "aFChunk";
        /// Comments (§11.3.2, §12.3.3, §13.3.2).
        COMMENTS = "comments";
        /// Document settings (§11.3.3).
        SETTINGS = "settings";
        /// Endnotes (§11.3.4).
        ENDNOTES = "endnotes";
        /// Font table (§11.3.5).
        FONT_TABLE = "fontTable";
        /// Footer (§11.3.6).
        FOOTER = "footer";
        /// Footnotes (§11.3.7).
        FOOTNOTES = "footnotes";
        /// Glossary document (§11.3.8).
        GLOSSARY_DOCUMENT = "glossaryDocument";
        /// Header (§11.3.9).
        HEADER = "header";
        /// Numbering definitions (§11.3.11).
        NUMBERING = "numbering";
        /// Style definitions (§11.3.12, §12.3.20).
        STYLES = "styles";
        /// Web settings (§11.3.13).
        WEB_SETTINGS = "webSettings";
        /// Calculation chain (§12.3.1).
        CALC_CHAIN = "calcChain";
        /// Chartsheet (§12.3.2).
        CHARTSHEET = "chartsheet";
        /// Connections (§12.3.4).
        CONNECTIONS = "connections";
        /// Custom property (§12.3.5).
        CUSTOM_PROPERTY = "customProperty";
        /// Custom XML mappings (§12.3.6).
        XML_MAPS = "xmlMaps";
        /// Dialogsheet (§12.3.7).
        DIALOGSHEET = "dialogsheet";
        /// Drawing (§12.3.8).
        DRAWING = "drawing";
        /// External workbook references (§12.3.9).
        EXTERNAL_LINK = "externalLink";
        /// Cell metadata (§12.3.10).
        SHEET_METADATA = "sheetMetadata";
        /// Pivot table (§12.3.11).
        PIVOT_TABLE = "pivotTable";
        /// Pivot cache definition (§12.3.12).
        PIVOT_CACHE_DEFINITION = "pivotCacheDefinition";
        /// Pivot cache records (§12.3.13).
        PIVOT_CACHE_RECORDS = "pivotCacheRecords";
        /// Query table (§12.3.14).
        QUERY_TABLE = "queryTable";
        /// Shared string table (§12.3.15).
        SHARED_STRINGS = "sharedStrings";
        /// Shared workbook revision headers (§12.3.16).
        REVISION_HEADERS = "revisionHeaders";
        /// Shared workbook revision log (§12.3.17).
        REVISION_LOG = "revisionLog";
        /// Shared workbook user data (§12.3.18).
        USERNAMES = "usernames";
        /// Single cell table definitions (§12.3.19).
        TABLE_SINGLE_CELLS = "tableSingleCells";
        /// Table definition (§12.3.21).
        TABLE = "table";
        /// Volatile dependencies (§12.3.22).
        VOLATILE_DEPENDENCIES = "volatileDependencies";
        /// Worksheet (§12.3.24).
        WORKSHEET = "worksheet";
        /// Comment authors (§13.3.1).
        COMMENT_AUTHORS = "commentAuthors";
        /// Handout master (§13.3.3).
        HANDOUT_MASTER = "handoutMaster";
        /// Notes master (§13.3.4).
        NOTES_MASTER = "notesMaster";
        /// Notes slide (§13.3.5).
        NOTES_SLIDE = "notesSlide";
        /// Presentation properties (§13.3.7).
        PRES_PROPS = "presProps";
        /// Slide (§13.3.8).
        SLIDE = "slide";
        /// Slide layout (§13.3.9).
        SLIDE_LAYOUT = "slideLayout";
        /// Slide master (§13.3.10).
        SLIDE_MASTER = "slideMaster";
        /// Slide synchronization data (§13.3.11).
        SLIDE_UPDATE_INFO = "slideUpdateInfo";
        /// User-defined tags (§13.3.12).
        TAGS = "tags";
        /// View properties (§13.3.13).
        VIEW_PROPS = "viewProps";
        /// Chart (§14.2.1).
        CHART = "chart";
        /// Chart drawing (§14.2.2).
        CHART_USER_SHAPES = "chartUserShapes";
        /// Diagram colors (§14.2.3).
        DIAGRAM_COLORS = "diagramColors";
        /// Diagram data (§14.2.4).
        DIAGRAM_DATA = "diagramData";
        /// Diagram layout definition (§14.2.5).
        DIAGRAM_LAYOUT = "diagramLayout";
        /// Diagram style (§14.2.6).
        DIAGRAM_QUICK_STYLE = "diagramQuickStyle";
        /// Theme (§14.2.7).
        THEME = "theme";
        /// Theme override (§14.2.8).
        THEME_OVERRIDE = "themeOverride";
        /// Table styles (§14.2.9).
        TABLE_STYLES = "tableStyles";
        /// Audio (§15.2.2).
        AUDIO = "audio";
        /// Custom XML data (§15.2.3–15.2.5).
        CUSTOM_XML = "customXml";
        /// Custom XML data properties (§15.2.6).
        CUSTOM_XML_PROPS = "customXmlProps";
        /// Embedded control (§15.2.9).
        CONTROL = "control";
        /// Embedded OLE object (§15.2.10).
        OLE_OBJECT = "oleObject";
        /// Embedded package (§15.2.11).
        PACKAGE = "package";
        /// Font (§15.2.13).
        FONT = "font";
        /// Image (§15.2.14).
        IMAGE = "image";
        /// Printer settings (§15.2.15).
        PRINTER_SETTINGS = "printerSettings";
        /// Video (§15.2.17).
        VIDEO = "video";
        /// Hyperlink (§15.3); always an external or fragment target.
        HYPERLINK = "hyperlink";
        /// Legacy VML drawing (Part 4).
        VML_DRAWING = "vmlDrawing";
    }
}

/// Content types.
pub mod content_types {
    /// Relationships part (Part 2).
    pub const RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";
    /// Core properties (Part 2).
    pub const CORE_PROPERTIES: &str = "application/vnd.openxmlformats-package.core-properties+xml";
    /// Generic XML.
    pub const XML: &str = "application/xml";
    /// Extended properties.
    pub const EXTENDED_PROPERTIES: &str =
        "application/vnd.openxmlformats-officedocument.extended-properties+xml";
    /// Custom properties.
    pub const CUSTOM_PROPERTIES: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
    /// Theme.
    pub const THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
    /// Theme override.
    pub const THEME_OVERRIDE: &str = "application/vnd.openxmlformats-officedocument.themeOverride+xml";
    /// Custom XML properties.
    pub const CUSTOM_XML_PROPERTIES: &str =
        "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
    /// DrawingML chart.
    pub const CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
    /// DrawingML chart drawing.
    pub const CHART_SHAPES: &str = "application/vnd.openxmlformats-officedocument.drawingml.chartshapes+xml";
    /// Diagram colors.
    pub const DIAGRAM_COLORS: &str =
        "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml";
    /// Diagram data.
    pub const DIAGRAM_DATA: &str = "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
    /// Diagram layout.
    pub const DIAGRAM_LAYOUT: &str =
        "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml";
    /// Diagram style.
    pub const DIAGRAM_STYLE: &str =
        "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml";
    /// SpreadsheetML / shared drawing.
    pub const DRAWING: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
    /// Legacy VML drawing.
    pub const VML_DRAWING: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

    /// WordprocessingML main document.
    pub const WML_DOCUMENT: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
    /// WordprocessingML template main document.
    pub const WML_TEMPLATE: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";
    /// Macro-enabled WordprocessingML main document.
    pub const WML_DOCUMENT_MACRO: &str = "application/vnd.ms-word.document.macroEnabled.main+xml";
    /// Macro-enabled WordprocessingML template.
    pub const WML_TEMPLATE_MACRO: &str = "application/vnd.ms-word.template.macroEnabledTemplate.main+xml";
    /// WordprocessingML styles.
    pub const WML_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
    /// WordprocessingML settings.
    pub const WML_SETTINGS: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
    /// WordprocessingML web settings.
    pub const WML_WEB_SETTINGS: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.webSettings+xml";
    /// WordprocessingML font table.
    pub const WML_FONT_TABLE: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";
    /// WordprocessingML numbering.
    pub const WML_NUMBERING: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
    /// WordprocessingML footnotes.
    pub const WML_FOOTNOTES: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
    /// WordprocessingML endnotes.
    pub const WML_ENDNOTES: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
    /// WordprocessingML comments.
    pub const WML_COMMENTS: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
    /// WordprocessingML header.
    pub const WML_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
    /// WordprocessingML footer.
    pub const WML_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
    /// WordprocessingML glossary document.
    pub const WML_GLOSSARY: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml";

    /// SpreadsheetML workbook.
    pub const SML_WORKBOOK: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
    /// SpreadsheetML template workbook.
    pub const SML_TEMPLATE: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml";
    /// Macro-enabled SpreadsheetML workbook.
    pub const SML_WORKBOOK_MACRO: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
    /// SpreadsheetML worksheet.
    pub const SML_WORKSHEET: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
    /// SpreadsheetML chartsheet.
    pub const SML_CHARTSHEET: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml";
    /// SpreadsheetML dialogsheet.
    pub const SML_DIALOGSHEET: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.dialogsheet+xml";
    /// SpreadsheetML shared strings.
    pub const SML_SHARED_STRINGS: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
    /// SpreadsheetML styles.
    pub const SML_STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
    /// SpreadsheetML calculation chain.
    pub const SML_CALC_CHAIN: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml";
    /// SpreadsheetML comments.
    pub const SML_COMMENTS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
    /// SpreadsheetML table.
    pub const SML_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";
    /// SpreadsheetML pivot table.
    pub const SML_PIVOT_TABLE: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
    /// SpreadsheetML pivot cache definition.
    pub const SML_PIVOT_CACHE_DEFINITION: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
    /// SpreadsheetML pivot cache records.
    pub const SML_PIVOT_CACHE_RECORDS: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml";
    /// SpreadsheetML external link.
    pub const SML_EXTERNAL_LINK: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml";
    /// SpreadsheetML cell metadata.
    pub const SML_SHEET_METADATA: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml";
    /// SpreadsheetML printer settings.
    pub const SML_PRINTER_SETTINGS: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings";

    /// PresentationML presentation.
    pub const PML_PRESENTATION: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
    /// PresentationML template.
    pub const PML_TEMPLATE: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml";
    /// PresentationML slide show.
    pub const PML_SLIDESHOW: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml";
    /// Macro-enabled presentation.
    pub const PML_PRESENTATION_MACRO: &str =
        "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml";
    /// PresentationML slide.
    pub const PML_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
    /// PresentationML slide layout.
    pub const PML_SLIDE_LAYOUT: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
    /// PresentationML slide master.
    pub const PML_SLIDE_MASTER: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
    /// PresentationML notes slide.
    pub const PML_NOTES_SLIDE: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
    /// PresentationML notes master.
    pub const PML_NOTES_MASTER: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
    /// PresentationML handout master.
    pub const PML_HANDOUT_MASTER: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.handoutMaster+xml";
    /// PresentationML presentation properties.
    pub const PML_PRES_PROPS: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";
    /// PresentationML view properties.
    pub const PML_VIEW_PROPS: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml";
    /// PresentationML table styles.
    pub const PML_TABLE_STYLES: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
    /// PresentationML comment authors.
    pub const PML_COMMENT_AUTHORS: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml";
    /// PresentationML comments.
    pub const PML_COMMENTS: &str =
        "application/vnd.openxmlformats-officedocument.presentationml.comments+xml";
    /// PresentationML tags.
    pub const PML_TAGS: &str = "application/vnd.openxmlformats-officedocument.presentationml.tags+xml";

    /// PNG image.
    pub const PNG: &str = "image/png";
    /// JPEG image.
    pub const JPEG: &str = "image/jpeg";
    /// GIF image.
    pub const GIF: &str = "image/gif";
    /// BMP image.
    pub const BMP: &str = "image/bmp";
    /// TIFF image.
    pub const TIFF: &str = "image/tiff";
    /// Enhanced metafile.
    pub const EMF: &str = "image/x-emf";
    /// Windows metafile.
    pub const WMF: &str = "image/x-wmf";
    /// SVG image.
    pub const SVG: &str = "image/svg+xml";

    /// The content type conventionally used for an image file extension.
    pub fn for_image_extension(ext: &str) -> Option<&'static str> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" => PNG,
            "jpg" | "jpeg" | "jpe" => JPEG,
            "gif" => GIF,
            "bmp" => BMP,
            "tif" | "tiff" => TIFF,
            "emf" => EMF,
            "wmf" => WMF,
            "svg" => SVG,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_relationship_types_are_canonicalised() {
        assert_eq!(
            canonical_relationship_type(
                "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument"
            ),
            rel_types::OFFICE_DOCUMENT
        );
        assert_eq!(canonical_relationship_type(rel_types::STYLES), rel_types::STYLES);
        assert!(matches!(
            canonical_relationship_type(rel_types::CORE_PROPERTIES),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn relationship_constants_are_spelled_correctly() {
        assert_eq!(
            rel_types::WORKSHEET,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"
        );
        assert_eq!(
            rel_types::SLIDE_LAYOUT,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout"
        );
    }

    #[test]
    fn image_content_types() {
        assert_eq!(content_types::for_image_extension("PNG"), Some("image/png"));
        assert_eq!(content_types::for_image_extension("jpg"), Some("image/jpeg"));
        assert_eq!(content_types::for_image_extension("txt"), None);
    }
}
