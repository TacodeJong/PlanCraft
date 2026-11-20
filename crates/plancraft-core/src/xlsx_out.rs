//! A minimal `.xlsx` workbook: inline strings, row outlines and one Excel table. No native code.
//!
//! Dates are written as text. The caller formats them.

use std::io::{Cursor, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// One cell. `indent` is the Excel alignment indent (outline depth). `bold` uses the bold font.
/// `number` writes a numeric cell (`<v>`) so a pivot can sum it; otherwise the text is inline.
pub struct XlsxCell {
    pub text: String,
    pub bold: bool,
    pub indent: u8,
    pub number: bool,
    /// Solid fill, `RRGGBB`. `None` keeps the cell unfilled.
    pub fill: Option<[u8; 3]>,
}

impl XlsxCell {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), bold: false, indent: 0, number: false, fill: None }
    }

    pub fn plain(text: impl Into<String>, bold: bool, indent: u8) -> Self {
        Self { text: text.into(), bold, indent, number: false, fill: None }
    }

    pub fn number(text: impl Into<String>) -> Self {
        Self { text: text.into(), bold: false, indent: 0, number: true, fill: None }
    }

    pub fn filled(text: impl Into<String>, bold: bool, rgb: [u8; 3]) -> Self {
        Self { text: text.into(), bold, indent: 0, number: false, fill: Some(rgb) }
    }
}

/// One sheet row. `outline` is the Excel row outline level (0–7).
pub struct XlsxRow {
    pub cells: Vec<XlsxCell>,
    pub outline: u8,
}

/// One worksheet. `table` marks the used range as an Excel table.
pub struct XlsxSheet {
    pub name: String,
    pub rows: Vec<XlsxRow>,
    pub table: bool,
    /// Frozen columns and rows. `(0, 0)` freezes nothing.
    pub freeze: (u32, u32),
}

/// One sheet named `Report`, with no outline and no table. Kept for the simple export test.
pub fn workbook(headers: &[String], rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    let mut all = vec![XlsxRow { cells: headers.iter().cloned().map(XlsxCell::text).collect(), outline: 0 }];
    all.extend(rows.iter().map(|r| XlsxRow { cells: r.iter().cloned().map(XlsxCell::text).collect(), outline: 0 }));
    sheets(&[XlsxSheet { name: "Report".into(), rows: all, table: false, freeze: (0, 0) }])
}

/// Writes every sheet. The first is `sheet1`, the second `sheet2`. A table sheet gets `table1`.
pub fn sheets(book: &[XlsxSheet]) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut out);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let table = book.iter().any(|s| s.table);
    write_part(&mut zip, &opts, "[Content_Types].xml", &content_types(book.len(), table))?;
    write_part(
        &mut zip,
        &opts,
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    )?;
    write_part(&mut zip, &opts, "xl/workbook.xml", &workbook_xml(book))?;
    write_part(&mut zip, &opts, "xl/_rels/workbook.xml.rels", &workbook_rels(book.len()))?;
    let fills = collect_fills(book);
    write_part(&mut zip, &opts, "xl/styles.xml", &styles_xml(&fills))?;
    for (i, sheet) in book.iter().enumerate() {
        let name = format!("xl/worksheets/sheet{}.xml", i.saturating_add(1));
        write_part(&mut zip, &opts, &name, &sheet_xml(sheet, &fills))?;
        if sheet.table {
            let rels = format!("xl/worksheets/_rels/sheet{}.xml.rels", i.saturating_add(1));
            write_part(
                &mut zip,
                &opts,
                &rels,
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#,
            )?;
            write_part(&mut zip, &opts, "xl/tables/table1.xml", &table_xml(sheet))?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn write_part(zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>, opts: &SimpleFileOptions, name: &str, body: &str) -> Result<(), String> {
    zip.start_file(name, *opts).map_err(|e| e.to_string())?;
    zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

fn content_types(n_sheets: usize, table: bool) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    for i in 0..n_sheets {
        body.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
            i.saturating_add(1)
        ));
    }
    if table {
        body.push_str(
            r#"<Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>"#,
        );
    }
    body.push_str("</Types>");
    body
}

fn workbook_xml(book: &[XlsxSheet]) -> String {
    let mut sheets = String::new();
    for (i, sheet) in book.iter().enumerate() {
        let id = i.saturating_add(1);
        sheets.push_str(&format!(r#"<sheet name="{}" sheetId="{id}" r:id="rId{id}"/>"#, xml(&sheet.name)));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#
    )
}

fn workbook_rels(n_sheets: usize) -> String {
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 0..n_sheets {
        let id = i.saturating_add(1);
        rels.push_str(&format!(
            r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{id}.xml"/>"#
        ));
    }
    let style = n_sheets.saturating_add(1);
    rels.push_str(&format!(
        r#"<Relationship Id="rId{style}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
    ));
    rels
}

fn collect_fills(book: &[XlsxSheet]) -> Vec<[u8; 3]> {
    let mut fills = Vec::new();
    for sheet in book {
        for row in &sheet.rows {
            for cell in &row.cells {
                if let Some(rgb) = cell.fill
                    && !fills.contains(&rgb)
                {
                    fills.push(rgb);
                }
            }
        }
    }
    fills
}

fn styles_xml(fills: &[[u8; 3]]) -> String {
    let mut xfs = String::new();
    let mut count = 0u32;
    for bold in [0u8, 1] {
        for indent in 0u8..=8 {
            xfs.push_str(&format!(r#"<xf fontId="{bold}" applyAlignment="1"><alignment horizontal="left" indent="{indent}"/></xf>"#));
            count = count.saturating_add(1);
        }
    }
    let mut fill_xml = String::from(r#"<fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>"#);
    for (i, rgb) in fills.iter().enumerate() {
        let hex = format!("FF{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
        fill_xml.push_str(&format!(
            r#"<fill><patternFill patternType="solid"><fgColor rgb="{hex}"/><bgColor rgb="{hex}"/></patternFill></fill>"#
        ));
        let id = 2 + i;
        for bold in [0u8, 1] {
            xfs.push_str(&format!(
                r#"<xf fontId="{bold}" fillId="{id}" applyFill="1" applyFont="1" applyAlignment="1"><alignment horizontal="center"/></xf>"#
            ));
            count = count.saturating_add(1);
        }
    }
    let fill_count = 2 + fills.len();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="{fill_count}">{fill_xml}</fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="{count}">{xfs}</cellXfs></styleSheet>"#
    )
}

fn sheet_xml(sheet: &XlsxSheet, fills: &[[u8; 3]]) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetPr><outlinePr summaryBelow="0"/></sheetPr>"#,
    );
    let (cols, rows) = sheet.freeze;
    if cols > 0 || rows > 0 {
        let top = format!("{}{}", col_name(cols as usize), rows.saturating_add(1));
        body.push_str(&format!(
            r#"<sheetViews><sheetView workbookViewId="0"><pane xSplit="{cols}" ySplit="{rows}" topLeftCell="{top}" activePane="bottomRight" state="frozen"/></sheetView></sheetViews>"#
        ));
    }
    body.push_str("<sheetData>");
    for (i, row) in sheet.rows.iter().enumerate() {
        write_row(&mut body, i, row, fills);
    }
    body.push_str("</sheetData>");
    if sheet.table {
        body.push_str(r#"<tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#);
    }
    body.push_str("</worksheet>");
    body
}

fn write_row(out: &mut String, row: usize, data: &XlsxRow, fills: &[[u8; 3]]) {
    let level = data.outline.min(7);
    if level > 0 {
        out.push_str(&format!(r#"<row r="{}" outlineLevel="{level}">"#, row.saturating_add(1)));
    } else {
        out.push_str(&format!(r#"<row r="{}">"#, row.saturating_add(1)));
    }
    for (col, cell) in data.cells.iter().enumerate() {
        let at = cell_ref(col, row);
        let style = style_index(cell, fills);
        if cell.number {
            out.push_str(&format!(r#"<c r="{at}" s="{style}"><v>{}</v></c>"#, xml(&cell.text)));
        } else {
            out.push_str(&format!(r#"<c r="{at}" s="{style}" t="inlineStr"><is><t>{}</t></is></c>"#, xml(&cell.text)));
        }
    }
    out.push_str("</row>");
}

fn style_index(cell: &XlsxCell, fills: &[[u8; 3]]) -> u32 {
    if let Some(rgb) = cell.fill
        && let Some(i) = fills.iter().position(|c| *c == rgb)
    {
        18 + (i as u32) * 2 + if cell.bold { 1 } else { 0 }
    } else {
        u32::from(cell.indent.min(8)).saturating_add(if cell.bold { 9 } else { 0 })
    }
}

fn table_xml(sheet: &XlsxSheet) -> String {
    let cols = sheet.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1).max(1);
    let rows = sheet.rows.len().max(1);
    let end = cell_ref(cols.saturating_sub(1), rows.saturating_sub(1));
    let range = format!("A1:{end}");
    let mut columns = String::new();
    let headers = sheet.rows.first().map(|r| r.cells.as_slice()).unwrap_or(&[]);
    for (i, cell) in headers.iter().enumerate() {
        let name = if cell.text.trim().is_empty() { format!("Column {}", i.saturating_add(1)) } else { cell.text.clone() };
        columns.push_str(&format!(r#"<tableColumn id="{}" name="{}"/>"#, i.saturating_add(1), xml(&name)));
    }
    if headers.is_empty() {
        columns.push_str(r#"<tableColumn id="1" name="Column 1"/>"#);
    }
    let count = headers.len().max(1);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Data" displayName="Data" ref="{range}" totalsRowCount="0"><autoFilter ref="{range}"/><tableColumns count="{count}">{columns}</tableColumns><tableStyleInfo name="TableStyleMedium2" showRowStripes="1"/></table>"#
    )
}

/// `A1:C4` covering the used rows and columns of a sheet (at least `A1`).
pub fn used_range(sheet: &XlsxSheet) -> String {
    let cols = sheet.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1).max(1);
    let rows = sheet.rows.len().max(1);
    format!("A1:{}", cell_ref(cols.saturating_sub(1), rows.saturating_sub(1)))
}

fn cell_ref(col: usize, row: usize) -> String {
    format!("{}{}", col_name(col), row.saturating_add(1))
}

fn col_name(mut n: usize) -> String {
    let mut out = String::new();
    loop {
        let rem = u8::try_from(n % 26).unwrap_or(0);
        out.insert(0, char::from(b'A'.saturating_add(rem)));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out
}

fn xml(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tasks_export_contains_header_and_names() {
        let headers = ["WBS", "Name", "Milestone", "Start", "Finish", "Duration", "Resources", "Predecessors", "% Complete", "Status"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let rows = vec![row("1", "Design", "15-10-2026"), row("2", "Build", "16-10-2026")];
        let bytes = workbook(&headers, &rows).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut xml).unwrap();
        assert!(xml.contains("Name"), "{xml}");
        assert!(xml.contains("Design"), "{xml}");
        assert!(xml.contains("Build"), "{xml}");
        assert!(xml.contains("15-10-2026"), "{xml}");
        assert!(xml.contains("16-10-2026"), "{xml}");
    }

    fn row(wbs: &str, name: &str, start: &str) -> Vec<String> {
        [wbs, name, "", start, start, "1d", "", "", "0", "Not started"].into_iter().map(str::to_string).collect()
    }
}
