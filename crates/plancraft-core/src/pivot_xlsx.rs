//! Inject a native Excel PivotTable into an existing `.xlsx` file.
//!
//! `rust_xlsxwriter` cannot emit the pivot OPC parts, so this module
//! post-processes the finished workbook (which is a zip archive) and adds:
//!
//! - `xl/pivotCache/pivotCacheDefinition1.xml` (+ its `.rels`)
//! - `xl/pivotCache/pivotCacheRecords1.xml` (empty, `count="0"`)
//! - `xl/pivotTables/pivotTable1.xml` (+ its `.rels`)
//! - a `<pivotCaches>` element in `xl/workbook.xml` plus the workbook
//!   relationship it points at
//! - `[Content_Types].xml` overrides for the three new parts
//! - a `pivotTable` relationship on the worksheet that hosts the pivot
//!
//! The cache is written with `saveData="0"` and `refreshOnLoad="1"`, the same
//! strategy the Go `excelize` library uses: Excel rebuilds the pivot cache
//! from the source range when the file is opened, so the records part can
//! stay empty. The consequence is that the pivot renders on first open in
//! Excel; viewers that never refresh caches (e.g. Quick Look) show an empty
//! pivot area.

use std::collections::HashMap;
use std::fmt;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Errors produced while injecting pivot-table parts into a workbook.
#[derive(Debug)]
pub enum PivotError {
    /// An underlying I/O failure while rebuilding the zip archive.
    Io(std::io::Error),
    /// The input was not a readable zip/xlsx archive.
    Zip(String),
    /// An XML part could not be parsed.
    Xml(String),
    /// A part was not valid UTF-8.
    Encoding(String),
    /// The `data_range` argument was not a valid `A1:K16` style range.
    InvalidRange(String),
    /// A sheet named in the arguments does not exist in the workbook.
    SheetNotFound(String),
    /// A level or value field name was not found in the header row.
    FieldNotFound(String),
    /// A required part (e.g. `xl/workbook.xml`) is missing from the archive.
    MissingPart(String),
    /// A required marker was missing while editing an existing XML part.
    MissingMarker(String),
    /// The workbook already contains pivot parts; injecting twice is not supported.
    AlreadyHasPivot,
}

impl fmt::Display for PivotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "i/o error: {e}"),
            Self::Zip(e) => write!(f, "zip error: {e}"),
            Self::Xml(e) => write!(f, "xml parse error: {e}"),
            Self::Encoding(e) => write!(f, "encoding error: {e}"),
            Self::InvalidRange(r) => write!(f, "invalid cell range: {r}"),
            Self::SheetNotFound(s) => write!(f, "sheet not found in workbook: {s}"),
            Self::FieldNotFound(s) => write!(f, "field not found in header row: {s}"),
            Self::MissingPart(p) => write!(f, "required part missing from xlsx: {p}"),
            Self::MissingMarker(m) => write!(f, "expected XML marker not found: {m}"),
            Self::AlreadyHasPivot => write!(f, "workbook already contains pivot table parts"),
        }
    }
}

impl std::error::Error for PivotError {}

impl From<std::io::Error> for PivotError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<zip::result::ZipError> for PivotError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Zip(e.to_string())
    }
}

/// A parsed cell value from the data sheet.
#[derive(Debug, Clone, PartialEq)]
enum CellValue {
    /// String content (shared, inline or formula string).
    Text(String),
    /// Numeric content, kept as the raw XML token to avoid float round-trips.
    Number(String),
}

impl CellValue {
    fn as_text(&self) -> &str {
        match self {
            Self::Text(s) | Self::Number(s) => s,
        }
    }
}

/// 1-based inclusive rectangular range on a sheet.
#[derive(Debug, Clone, Copy)]
struct Range {
    first_row: u32,
    first_col: u32,
    last_row: u32,
    last_col: u32,
}

/// Inject a native PivotTable into a finished `.xlsx` workbook.
///
/// * `xlsx_bytes` - the full workbook file, e.g. from `Workbook::save_to_buffer`.
/// * `data_sheet` - name of the sheet holding the flat source data.
/// * `data_range` - `A1:K16` style range **including the header row**.
/// * `pivot_sheet` - name of the (typically empty) sheet that will host the pivot.
/// * `levels` - header names used as nested row fields, outermost first.
/// * `value_field` - header name summed in the values area.
///
/// Returns the rewritten workbook bytes. The pivot cache is marked
/// `refreshOnLoad`, so Excel populates the pivot when the file is opened.
pub fn inject_pivot_table(
    xlsx_bytes: Vec<u8>,
    data_sheet: &str,
    data_range: &str,
    pivot_sheet: &str,
    levels: &[&str],
    value_field: &str,
) -> Result<Vec<u8>, PivotError> {
    let mut parts = read_zip_parts(xlsx_bytes)?;

    if parts.iter().any(|(name, _)| name.starts_with("xl/pivotCache/") || name.starts_with("xl/pivotTables/")) {
        return Err(PivotError::AlreadyHasPivot);
    }

    let range = parse_range(data_range)?;

    // Resolve sheet names to their part paths via workbook.xml + its rels.
    let workbook_xml = part_as_string(&parts, "xl/workbook.xml")?;
    if workbook_xml.contains("<pivotCaches>") {
        return Err(PivotError::AlreadyHasPivot);
    }
    let workbook_rels = part_as_string(&parts, "xl/_rels/workbook.xml.rels")?;
    let sheets = parse_workbook_sheets(&workbook_xml)?;
    let rels = parse_relationships(&workbook_rels)?;

    let data_sheet_path = sheet_part_path(&sheets, &rels, data_sheet).ok_or_else(|| PivotError::SheetNotFound(data_sheet.to_string()))?;
    let pivot_sheet_path =
        sheet_part_path(&sheets, &rels, pivot_sheet).ok_or_else(|| PivotError::SheetNotFound(pivot_sheet.to_string()))?;

    // Read the source data so we can build real cache fields / shared items.
    let shared_strings = match find_part(&parts, "xl/sharedStrings.xml") {
        Some(bytes) => parse_shared_strings(&to_utf8(bytes)?)?,
        None => Vec::new(),
    };
    let sheet_xml = part_as_string(&parts, &data_sheet_path)?;
    let cells = parse_sheet_cells(&sheet_xml, range, &shared_strings)?;

    // Header row and field indices.
    let mut headers: Vec<String> = Vec::new();
    for col in range.first_col..=range.last_col {
        let header = cells.get(&(range.first_row, col)).map(|v| v.as_text().to_string()).unwrap_or_default();
        if header.is_empty() {
            return Err(PivotError::InvalidRange(format!("empty header cell in column {col} of range {data_range}")));
        }
        headers.push(header);
    }
    let field_index = |name: &str| -> Result<usize, PivotError> {
        headers.iter().position(|h| h == name).ok_or_else(|| PivotError::FieldNotFound(name.to_string()))
    };
    let mut level_indices: Vec<usize> = Vec::new();
    for level in levels {
        level_indices.push(field_index(level)?);
    }
    let value_index = field_index(value_field)?;

    // Per-column data (rows below the header), in range order.
    let columns: Vec<Vec<Option<CellValue>>> = (range.first_col..=range.last_col)
        .map(|col| ((range.first_row + 1)..=range.last_row).map(|row| cells.get(&(row, col)).cloned()).collect())
        .collect();

    // Build the new parts.
    let cache_definition = build_cache_definition(data_sheet, data_range, &headers, &columns, &level_indices);
    let cache_records = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<pivotCacheRecords xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" ",
        "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" count=\"0\"/>"
    )
    .to_string();
    let pivot_table = build_pivot_table(&headers, &columns, &level_indices, value_index, value_field);

    let cache_definition_rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords\" Target=\"pivotCacheRecords1.xml\"/>",
        "</Relationships>"
    )
    .to_string();
    let pivot_table_rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition\" Target=\"../pivotCache/pivotCacheDefinition1.xml\"/>",
        "</Relationships>"
    )
    .to_string();

    // 1. workbook rels: relationship from the workbook to the cache definition.
    let cache_rid = format!("rId{}", next_relationship_id(&workbook_rels));
    let new_workbook_rels = insert_before(
        workbook_rels,
        "</Relationships>",
        &format!(
            "<Relationship Id=\"{cache_rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition\" Target=\"pivotCache/pivotCacheDefinition1.xml\"/>"
        ),
    )?;

    // 2. workbook.xml: pivotCaches element (valid after calcPr, before </workbook>).
    let new_workbook_xml = insert_before(
        workbook_xml,
        "</workbook>",
        &format!("<pivotCaches><pivotCache cacheId=\"1\" r:id=\"{cache_rid}\"/></pivotCaches>"),
    )?;

    // 3. [Content_Types].xml overrides.
    let content_types = part_as_string(&parts, "[Content_Types].xml")?;
    let new_content_types = insert_before(
        content_types,
        "</Types>",
        concat!(
            "<Override PartName=\"/xl/pivotTables/pivotTable1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml\"/>",
            "<Override PartName=\"/xl/pivotCache/pivotCacheDefinition1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml\"/>",
            "<Override PartName=\"/xl/pivotCache/pivotCacheRecords1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml\"/>"
        ),
    )?;

    // 4. Worksheet rels for the pivot sheet (create or extend).
    let pivot_sheet_rels_path = worksheet_rels_path(&pivot_sheet_path);
    let pivot_sheet_rel_type = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable";
    let new_pivot_sheet_rels = match find_part(&parts, &pivot_sheet_rels_path) {
        Some(bytes) => {
            let existing = to_utf8(bytes)?;
            let rid = next_relationship_id(&existing);
            insert_before(
                existing,
                "</Relationships>",
                &format!("<Relationship Id=\"rId{rid}\" Type=\"{pivot_sheet_rel_type}\" Target=\"../pivotTables/pivotTable1.xml\"/>"),
            )?
        }
        None => format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"{}\" Target=\"../pivotTables/pivotTable1.xml\"/>",
                "</Relationships>"
            ),
            pivot_sheet_rel_type
        ),
    };

    // Apply modifications and append new parts.
    replace_part(&mut parts, "xl/workbook.xml", new_workbook_xml);
    replace_part(&mut parts, "xl/_rels/workbook.xml.rels", new_workbook_rels);
    replace_part(&mut parts, "[Content_Types].xml", new_content_types);
    replace_part(&mut parts, &pivot_sheet_rels_path, new_pivot_sheet_rels);
    parts.push(("xl/pivotCache/pivotCacheDefinition1.xml".to_string(), cache_definition.into_bytes()));
    parts.push(("xl/pivotCache/_rels/pivotCacheDefinition1.xml.rels".to_string(), cache_definition_rels.into_bytes()));
    parts.push(("xl/pivotCache/pivotCacheRecords1.xml".to_string(), cache_records.into_bytes()));
    parts.push(("xl/pivotTables/pivotTable1.xml".to_string(), pivot_table.into_bytes()));
    parts.push(("xl/pivotTables/_rels/pivotTable1.xml.rels".to_string(), pivot_table_rels.into_bytes()));

    write_zip_parts(parts)
}

// ---------------------------------------------------------------------------
// XML part builders
// ---------------------------------------------------------------------------

/// Build `xl/pivotCache/pivotCacheDefinition1.xml`.
///
/// Row-field columns get fully enumerated `sharedItems` (the pivot table part
/// references them by index). Numeric columns get type-attribute-only
/// `sharedItems`. Everything else gets the excelize-style blank placeholder,
/// which is sufficient because the cache is rebuilt on load.
fn build_cache_definition(
    data_sheet: &str,
    data_range: &str,
    headers: &[String],
    columns: &[Vec<Option<CellValue>>],
    level_indices: &[usize],
) -> String {
    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str(concat!(
        "<pivotCacheDefinition xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" ",
        "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
        "r:id=\"rId1\" saveData=\"0\" refreshOnLoad=\"1\" ",
        "createdVersion=\"3\" refreshedVersion=\"3\" minRefreshableVersion=\"3\">"
    ));
    xml.push_str(&format!(
        "<cacheSource type=\"worksheet\"><worksheetSource ref=\"{}\" sheet=\"{}\"/></cacheSource>",
        xml_escape(data_range),
        xml_escape(data_sheet)
    ));
    xml.push_str(&format!("<cacheFields count=\"{}\">", headers.len()));
    for (idx, header) in headers.iter().enumerate() {
        let column = columns.get(idx);
        xml.push_str(&format!("<cacheField name=\"{}\" numFmtId=\"0\">", xml_escape(header)));
        if level_indices.contains(&idx) {
            xml.push_str(&shared_items_enumerated(column));
        } else {
            xml.push_str(&shared_items_summary(column));
        }
        xml.push_str("</cacheField>");
    }
    xml.push_str("</cacheFields>");
    xml.push_str("</pivotCacheDefinition>");
    xml
}

/// Distinct values of a column in first-appearance order. A `None` entry
/// (blank cell) is represented once as `None`.
fn distinct_values(column: Option<&Vec<Option<CellValue>>>) -> Vec<Option<CellValue>> {
    let mut distinct: Vec<Option<CellValue>> = Vec::new();
    if let Some(cells) = column {
        for cell in cells {
            if !distinct.contains(cell) {
                distinct.push(cell.clone());
            }
        }
    }
    distinct
}

/// `<sharedItems>` with every distinct value listed, for row-field columns.
fn shared_items_enumerated(column: Option<&Vec<Option<CellValue>>>) -> String {
    let distinct = distinct_values(column);
    let has_blank = distinct.iter().any(Option::is_none);
    let has_text = distinct.iter().any(|v| matches!(v, Some(CellValue::Text(_))));
    let has_number = distinct.iter().any(|v| matches!(v, Some(CellValue::Number(_))));
    let mut xml = String::from("<sharedItems");
    if has_blank {
        xml.push_str(" containsBlank=\"1\"");
    }
    if has_text && has_number {
        xml.push_str(" containsMixedTypes=\"1\"");
    }
    if has_number {
        xml.push_str(" containsNumber=\"1\"");
    }
    xml.push_str(&format!(" count=\"{}\">", distinct.len()));
    for value in &distinct {
        match value {
            Some(CellValue::Text(s)) => xml.push_str(&format!("<s v=\"{}\"/>", xml_escape(s))),
            Some(CellValue::Number(n)) => xml.push_str(&format!("<n v=\"{}\"/>", xml_escape(n))),
            None => xml.push_str("<m/>"),
        }
    }
    xml.push_str("</sharedItems>");
    xml
}

/// `<sharedItems>` for columns that are not row fields: numeric columns get a
/// type summary, everything else the blank placeholder used by excelize.
fn shared_items_summary(column: Option<&Vec<Option<CellValue>>>) -> String {
    let empty = Vec::new();
    let cells = column.unwrap_or(&empty);
    let has_blank = cells.iter().any(Option::is_none);
    let numbers: Vec<f64> = cells
        .iter()
        .filter_map(|c| match c {
            Some(CellValue::Number(n)) => n.parse::<f64>().ok(),
            _ => None,
        })
        .collect();
    let all_numeric = !numbers.is_empty() && cells.iter().all(|c| !matches!(c, Some(CellValue::Text(_)))) && !has_blank;
    if all_numeric {
        let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
        let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let integer = numbers.iter().all(|n| n.fract() == 0.0);
        let mut xml = String::from("<sharedItems containsSemiMixedTypes=\"0\" containsString=\"0\" containsNumber=\"1\"");
        if integer {
            xml.push_str(" containsInteger=\"1\"");
        }
        xml.push_str(&format!(" minValue=\"{min}\" maxValue=\"{max}\"/>"));
        xml
    } else {
        "<sharedItems containsBlank=\"1\" count=\"1\"><m/></sharedItems>".to_string()
    }
}

/// Build `xl/pivotTables/pivotTable1.xml`.
fn build_pivot_table(
    headers: &[String],
    columns: &[Vec<Option<CellValue>>],
    level_indices: &[usize],
    value_index: usize,
    value_field: &str,
) -> String {
    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str(concat!(
        "<pivotTableDefinition xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" ",
        "name=\"PivotTable1\" cacheId=\"1\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" ",
        "applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" ",
        "applyWidthHeightFormats=\"1\" dataCaption=\"Values\" updatedVersion=\"3\" ",
        "minRefreshableVersion=\"3\" createdVersion=\"3\" useAutoFormatting=\"1\" ",
        "itemPrintTitles=\"1\" indent=\"0\" outline=\"1\" outlineData=\"1\" multipleFieldFilters=\"0\">"
    ));
    xml.push_str("<location ref=\"A3:B20\" firstHeaderRow=\"1\" firstDataRow=\"1\" firstDataCol=\"1\"/>");
    xml.push_str(&format!("<pivotFields count=\"{}\">", headers.len()));
    for idx in 0..headers.len() {
        if level_indices.contains(&idx) {
            let item_count = distinct_values(columns.get(idx)).len();
            xml.push_str("<pivotField axis=\"axisRow\" showAll=\"0\">");
            xml.push_str(&format!("<items count=\"{}\">", item_count + 1));
            for item in 0..item_count {
                xml.push_str(&format!("<item x=\"{item}\"/>"));
            }
            xml.push_str("<item t=\"default\"/></items></pivotField>");
        } else if idx == value_index {
            xml.push_str("<pivotField dataField=\"1\" showAll=\"0\"/>");
        } else {
            xml.push_str("<pivotField showAll=\"0\"/>");
        }
    }
    xml.push_str("</pivotFields>");
    xml.push_str(&format!("<rowFields count=\"{}\">", level_indices.len()));
    for idx in level_indices {
        xml.push_str(&format!("<field x=\"{idx}\"/>"));
    }
    xml.push_str("</rowFields>");
    xml.push_str("<rowItems count=\"1\"><i><x/></i></rowItems>");
    xml.push_str("<colItems count=\"1\"><i/></colItems>");
    xml.push_str(&format!(
        "<dataFields count=\"1\"><dataField name=\"{}\" fld=\"{}\" baseField=\"0\" baseItem=\"0\"/></dataFields>",
        xml_escape(&format!("Sum of {value_field}")),
        value_index
    ));
    xml.push_str(concat!(
        "<pivotTableStyleInfo name=\"PivotStyleLight16\" showRowHeaders=\"1\" ",
        "showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/>"
    ));
    xml.push_str("</pivotTableDefinition>");
    xml
}

// ---------------------------------------------------------------------------
// Zip plumbing
// ---------------------------------------------------------------------------

fn read_zip_parts(bytes: Vec<u8>) -> Result<Vec<(String, Vec<u8>)>, PivotError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }
        let name = file.name()?.into_owned();
        let mut contents = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        file.read_to_end(&mut contents)?;
        parts.push((name, contents));
    }
    Ok(parts)
}

fn write_zip_parts(parts: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, PivotError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    // Stored, so callers that scan the raw workbook bytes still see sheet text and part names.
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, contents) in parts {
        writer.start_file(name, options)?;
        writer.write_all(&contents)?;
    }
    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}

fn find_part<'a>(parts: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    parts.iter().find(|(part, _)| part == name).map(|(_, bytes)| bytes.as_slice())
}

fn part_as_string(parts: &[(String, Vec<u8>)], name: &str) -> Result<String, PivotError> {
    match find_part(parts, name) {
        Some(bytes) => to_utf8(bytes),
        None => Err(PivotError::MissingPart(name.to_string())),
    }
}

fn replace_part(parts: &mut Vec<(String, Vec<u8>)>, name: &str, contents: String) {
    if let Some(entry) = parts.iter_mut().find(|(part, _)| part == name) {
        entry.1 = contents.into_bytes();
    } else {
        parts.push((name.to_string(), contents.into_bytes()));
    }
}

fn to_utf8(bytes: &[u8]) -> Result<String, PivotError> {
    String::from_utf8(bytes.to_vec()).map_err(|e| PivotError::Encoding(e.to_string()))
}

fn event_text(text: &quick_xml::events::BytesText<'_>) -> Result<String, PivotError> {
    let decoded = text.decode().map_err(|e| PivotError::Xml(e.to_string()))?;
    quick_xml::escape::unescape(&decoded).map(|s| s.into_owned()).map_err(|e| PivotError::Xml(e.to_string()))
}

// ---------------------------------------------------------------------------
// Workbook / worksheet XML parsing
// ---------------------------------------------------------------------------

/// Sheet (name, r:id) pairs from `xl/workbook.xml`, in workbook order.
fn parse_workbook_sheets(workbook_xml: &str) -> Result<Vec<(String, String)>, PivotError> {
    let mut reader = Reader::from_str(workbook_xml);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"sheet" {
                    let mut name = None;
                    let mut rid = None;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|err| PivotError::Xml(err.to_string()))?
                            .into_owned();
                        match attr.key.as_ref() {
                            b"name" => name = Some(value),
                            b"r:id" => rid = Some(value),
                            _ => {}
                        }
                    }
                    if let (Some(name), Some(rid)) = (name, rid) {
                        sheets.push((name, rid));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(PivotError::Xml(e.to_string())),
        }
    }
    Ok(sheets)
}

/// (Id, Target) pairs from a `.rels` part.
fn parse_relationships(rels_xml: &str) -> Result<Vec<(String, String)>, PivotError> {
    let mut reader = Reader::from_str(rels_xml);
    let mut rels = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut id = None;
                    let mut target = None;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|err| PivotError::Xml(err.to_string()))?
                            .into_owned();
                        match attr.key.as_ref() {
                            b"Id" => id = Some(value),
                            b"Target" => target = Some(value),
                            _ => {}
                        }
                    }
                    if let (Some(id), Some(target)) = (id, target) {
                        rels.push((id, target));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(PivotError::Xml(e.to_string())),
        }
    }
    Ok(rels)
}

/// Resolve a sheet name to its zip part path (e.g. `xl/worksheets/sheet2.xml`).
fn sheet_part_path(sheets: &[(String, String)], rels: &[(String, String)], sheet_name: &str) -> Option<String> {
    let rid = sheets.iter().find(|(name, _)| name == sheet_name).map(|(_, rid)| rid)?;
    let target = rels.iter().find(|(id, _)| id == rid).map(|(_, target)| target)?;
    if let Some(absolute) = target.strip_prefix('/') { Some(absolute.to_string()) } else { Some(format!("xl/{target}")) }
}

/// `xl/worksheets/sheet2.xml` -> `xl/worksheets/_rels/sheet2.xml.rels`
fn worksheet_rels_path(sheet_path: &str) -> String {
    match sheet_path.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{sheet_path}.rels"),
    }
}

/// Smallest unused `rIdN` number in a `.rels` part.
fn next_relationship_id(rels_xml: &str) -> u32 {
    let mut max_id = 0u32;
    for (pos, _) in rels_xml.match_indices("Id=\"rId") {
        let digits: String = rels_xml.get(pos + 7..).unwrap_or("").chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse::<u32>() {
            max_id = max_id.max(n);
        }
    }
    max_id + 1
}

/// All string items from `xl/sharedStrings.xml` (rich-text runs concatenated).
fn parse_shared_strings(xml: &str) -> Result<Vec<String>, PivotError> {
    let mut reader = Reader::from_str(xml);
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"si" => {
                    in_si = true;
                    current.clear();
                }
                b"t" if in_si => in_t = true,
                _ => {}
            },
            Ok(Event::Text(t)) if in_t => {
                let text = event_text(&t)?;
                current.push_str(&text);
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"t" => in_t = false,
                b"si" => {
                    in_si = false;
                    strings.push(current.clone());
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(PivotError::Xml(e.to_string())),
        }
    }
    Ok(strings)
}

/// Parse all cells of a worksheet that fall inside `range`.
fn parse_sheet_cells(xml: &str, range: Range, shared_strings: &[String]) -> Result<HashMap<(u32, u32), CellValue>, PivotError> {
    let mut reader = Reader::from_str(xml);
    let mut cells = HashMap::new();
    let mut current_ref: Option<(u32, u32)> = None;
    let mut current_type = String::new();
    let mut current_text: Option<String> = None;
    let mut in_v = false;
    let mut in_is_t = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"c" => {
                current_ref = None;
                current_type.clear();
                current_text = None;
                for attr in e.attributes().with_checks(false).flatten() {
                    let value =
                        attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|err| PivotError::Xml(err.to_string()))?;
                    match attr.key.as_ref() {
                        b"r" => current_ref = parse_cell_ref(&value),
                        b"t" => current_type = value.into_owned(),
                        _ => {}
                    }
                }
            }
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"v" if current_ref.is_some() => in_v = true,
                b"t" if current_ref.is_some() => in_is_t = true,
                _ => {}
            },
            Ok(Event::Text(t)) if in_v || in_is_t => {
                let text = event_text(&t)?;
                match current_text.as_mut() {
                    Some(existing) => existing.push_str(&text),
                    None => current_text = Some(text),
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"v" => in_v = false,
                b"t" => in_is_t = false,
                b"c" => {
                    if let (Some((row, col)), Some(raw)) = (current_ref, current_text.take()) {
                        let in_range = row >= range.first_row && row <= range.last_row && col >= range.first_col && col <= range.last_col;
                        if in_range {
                            let value = match current_type.as_str() {
                                "s" => raw.parse::<usize>().ok().and_then(|i| shared_strings.get(i)).map(|s| CellValue::Text(s.clone())),
                                "str" | "inlineStr" => Some(CellValue::Text(raw)),
                                "b" => Some(CellValue::Text(if raw == "1" { "TRUE" } else { "FALSE" }.to_string())),
                                _ => Some(CellValue::Number(raw)),
                            };
                            if let Some(value) = value
                                && !value.as_text().is_empty()
                            {
                                cells.insert((row, col), value);
                            }
                        }
                        current_ref = None;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(PivotError::Xml(e.to_string())),
        }
    }
    Ok(cells)
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// `"B7"` -> `(7, 2)` (1-based row, col).
fn parse_cell_ref(cell: &str) -> Option<(u32, u32)> {
    let letters: String = cell.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits: String = cell.chars().skip_while(char::is_ascii_alphabetic).collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col = 0u32;
    for ch in letters.chars() {
        let upper = ch.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        col = col.checked_mul(26)?.checked_add(u32::from(upper) - u32::from('A') + 1)?;
    }
    let row = digits.parse::<u32>().ok()?;
    Some((row, col))
}

fn parse_range(range: &str) -> Result<Range, PivotError> {
    let invalid = || PivotError::InvalidRange(range.to_string());
    let cleaned = range.replace('$', "");
    let (start, end) = cleaned.split_once(':').ok_or_else(invalid)?;
    let (first_row, first_col) = parse_cell_ref(start).ok_or_else(invalid)?;
    let (last_row, last_col) = parse_cell_ref(end).ok_or_else(invalid)?;
    if first_row >= last_row || first_col > last_col {
        return Err(invalid());
    }
    Ok(Range { first_row, first_col, last_row, last_col })
}

fn insert_before(haystack: String, marker: &str, insert: &str) -> Result<String, PivotError> {
    if !haystack.contains(marker) {
        return Err(PivotError::MissingMarker(marker.to_string()));
    }
    Ok(haystack.replace(marker, &format!("{insert}{marker}")))
}

fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
