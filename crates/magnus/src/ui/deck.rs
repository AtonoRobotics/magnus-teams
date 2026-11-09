//! Slides drawn by deterministic code: the agent fills the template's slots
//! and these write the file. PPTX is a zip of XML; PDF is written directly.
use serde_json::Value;

const SLIDE_WIDTH_EMU: u64 = 12_192_000;
const SLIDE_HEIGHT_EMU: u64 = 6_858_000;
const MARGIN_EMU: u64 = 600_000;
const PAGE_WIDTH_PT: u32 = 960;
const PAGE_HEIGHT_PT: u32 = 540;
const PAGE_MARGIN_PT: u32 = 48;
const TITLE_SIZE_PT: u32 = 32;
const BODY_SIZE_PT: u32 = 20;
const LINE_GAP_PT: u32 = 10;
/// Rough characters that fit a slide's width at body size.
const WRAP_CHARS: usize = 78;

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

/// A slide as a heading and lines of body, whatever its layout.
fn plain(slide: &Value) -> (String, Vec<String>) {
    let items = || slide["items"].as_array().map_or(vec![], |a| a.iter().filter_map(|i| i.as_str().map(String::from)).collect());
    match str_of(slide, "layout") {
        "title" => (str_of(slide, "title").to_string(), vec![str_of(slide, "subtitle").to_string()]),
        "bullets" => (str_of(slide, "title").to_string(), items().into_iter().map(|i| format!("\u{2022} {i}")).collect()),
        "quote" => (String::new(), vec![format!("\u{201c}{}\u{201d}", str_of(slide, "text")), str_of(slide, "subtitle").to_string()]),
        "image" => (str_of(slide, "title").to_string(), vec![format!("[{}]", str_of(slide, "file"))]),
        _ => (str_of(slide, "title").to_string(), str_of(slide, "text").split('\n').map(String::from).collect()),
    }
}

fn slides_of(deck: &Value) -> &[Value] {
    deck["slides"].as_array().map_or(&[], Vec::as_slice)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A zip with nothing compressed, which every reader accepts.
fn zip(entries: &[(String, String)]) -> Vec<u8> {
    let (mut out, mut central, count) = (Vec::new(), Vec::new(), entries.len() as u16);
    for (name, body) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(body.as_bytes());
        let size = body.len() as u32;
        let shared = |sig: &[u8], extra: &[u8], to: &mut Vec<u8>| {
            to.extend_from_slice(sig);
            to.extend_from_slice(extra);
            to.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 33, 0]);
            for v in [crc, size, size] {
                to.extend_from_slice(&v.to_le_bytes());
            }
            to.extend_from_slice(&(name.len() as u16).to_le_bytes());
            to.extend_from_slice(&[0, 0]);
        };
        shared(&[0x50, 0x4b, 3, 4], &[], &mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body.as_bytes());
        shared(&[0x50, 0x4b, 1, 2], &[20, 0], &mut central);
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const GROUP: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#;

fn rels(items: &[(&str, String)]) -> String {
    let all: String = items.iter().enumerate().map(|(i, (kind, to))| format!(r#"<Relationship Id="rId{}" Type="{REL}/{kind}" Target="{to}"/>"#, i + 1)).collect();
    format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{all}</Relationships>"#)
}

fn text_box(id: u32, y: u64, height: u64, size_pt: u32, bold: bool, lines: &[String]) -> String {
    let paragraphs: String =
        lines.iter().map(|l| format!(r#"<a:p><a:r><a:rPr lang="en-US" sz="{}" b="{}"/><a:t>{}</a:t></a:r></a:p>"#, size_pt * 100, u8::from(bold), xml(l))).collect();
    let width = SLIDE_WIDTH_EMU - 2 * MARGIN_EMU;
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{MARGIN_EMU}" y="{y}"/><a:ext cx="{width}" cy="{height}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square"/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#
    )
}

fn slide_xml(slide: &Value) -> String {
    let (title, body) = plain(slide);
    let heading = text_box(2, MARGIN_EMU, 1_200_000, TITLE_SIZE_PT, true, &[title]);
    let rest = text_box(3, MARGIN_EMU + 1_400_000, SLIDE_HEIGHT_EMU - 2 * MARGIN_EMU - 1_400_000, BODY_SIZE_PT, false, &body);
    format!(r#"{HEAD}<p:sld {NS}><p:cSld><p:spTree>{GROUP}{heading}{rest}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#)
}

const THEME_COLORS: &str = r#"<a:dk1><a:srgbClr val="1C1C1E"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="3F3F44"/></a:dk2><a:lt2><a:srgbClr val="F6F6F3"/></a:lt2><a:accent1><a:srgbClr val="3B5BDB"/></a:accent1><a:accent2><a:srgbClr val="C2410C"/></a:accent2><a:accent3><a:srgbClr val="0F766E"/></a:accent3><a:accent4><a:srgbClr val="7C3AED"/></a:accent4><a:accent5><a:srgbClr val="A16207"/></a:accent5><a:accent6><a:srgbClr val="BE123C"/></a:accent6><a:hlink><a:srgbClr val="3B5BDB"/></a:hlink><a:folHlink><a:srgbClr val="7C3AED"/></a:folHlink>"#;
const THEME_FONT: &str = r#"<a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/>"#;
const FILL: &str = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
const LINE: &str = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;

fn theme() -> String {
    format!(
        r#"{HEAD}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Magnus"><a:themeElements><a:clrScheme name="Magnus">{THEME_COLORS}</a:clrScheme><a:fontScheme name="Magnus"><a:majorFont>{THEME_FONT}</a:majorFont><a:minorFont>{THEME_FONT}</a:minorFont></a:fontScheme><a:fmtScheme name="Magnus"><a:fillStyleLst>{FILL}{FILL}{FILL}</a:fillStyleLst><a:lnStyleLst>{LINE}{LINE}{LINE}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{FILL}{FILL}{FILL}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

pub fn pptx(deck: &Value) -> Vec<u8> {
    let slides = slides_of(deck);
    let n = slides.len();
    let slide_types: String = (1..=n)
        .map(|i| format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
        .collect();
    let types = format!(
        r#"{HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{slide_types}</Types>"#
    );
    let ids: String = (1..=n).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 1)).collect();
    let presentation = format!(
        r#"{HEAD}<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{SLIDE_WIDTH_EMU}" cy="{SLIDE_HEIGHT_EMU}"/><p:notesSz cx="{SLIDE_HEIGHT_EMU}" cy="{SLIDE_WIDTH_EMU}"/></p:presentation>"#
    );
    let mut presentation_rels = vec![("slideMaster", "slideMasters/slideMaster1.xml".to_string())];
    presentation_rels.extend((1..=n).map(|i| ("slide", format!("slides/slide{i}.xml"))));
    let tree = format!("<p:cSld><p:spTree>{GROUP}</p:spTree></p:cSld>");
    let map = r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#;
    let mut files = vec![
        ("[Content_Types].xml".to_string(), types),
        ("_rels/.rels".to_string(), rels(&[("officeDocument", "ppt/presentation.xml".to_string())])),
        ("ppt/presentation.xml".to_string(), presentation),
        ("ppt/_rels/presentation.xml.rels".to_string(), rels(&presentation_rels)),
        ("ppt/theme/theme1.xml".to_string(), theme()),
        (
            "ppt/slideMasters/slideMaster1.xml".to_string(),
            format!(r#"{HEAD}<p:sldMaster {NS}>{tree}{map}<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#),
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(),
            rels(&[("slideLayout", "../slideLayouts/slideLayout1.xml".to_string()), ("theme", "../theme/theme1.xml".to_string())]),
        ),
        ("ppt/slideLayouts/slideLayout1.xml".to_string(), format!(r#"{HEAD}<p:sldLayout {NS} type="blank">{tree}<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#)),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(), rels(&[("slideMaster", "../slideMasters/slideMaster1.xml".to_string())])),
    ];
    for (i, s) in slides.iter().enumerate() {
        files.push((format!("ppt/slides/slide{}.xml", i + 1), slide_xml(s)));
        files.push((format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), rels(&[("slideLayout", "../slideLayouts/slideLayout1.xml".to_string())])));
    }
    zip(&files)
}

/// Text for a PDF string: its own escapes, and Latin-1 where it can be.
fn pdf_text(s: &str) -> Vec<u8> {
    let mut out = vec![];
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => out.extend([b'\\', c as u8]),
            '\u{2022}' => out.push(0x95),
            '\u{201c}' => out.push(0x93),
            '\u{201d}' => out.push(0x94),
            c if (c as u32) < 256 => out.push(c as u32 as u8),
            _ => out.push(b'?'),
        }
    }
    out
}

fn wrapped(line: &str) -> Vec<String> {
    let (mut out, mut current) = (vec![], String::new());
    for word in line.split(' ') {
        if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > WRAP_CHARS {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    out.push(current);
    out
}

fn page_stream(slide: &Value) -> Vec<u8> {
    let (title, body) = plain(slide);
    let mut out = Vec::new();
    let mut y = PAGE_HEIGHT_PT - PAGE_MARGIN_PT - TITLE_SIZE_PT;
    let put = |text: &str, font: &str, size: u32, y: u32, out: &mut Vec<u8>| {
        out.extend_from_slice(format!("BT /{font} {size} Tf {PAGE_MARGIN_PT} {y} Td (").as_bytes());
        out.extend(pdf_text(text));
        out.extend_from_slice(b") Tj ET\n");
    };
    if !title.is_empty() {
        put(&title, "F2", TITLE_SIZE_PT, y, &mut out);
        y = y.saturating_sub(TITLE_SIZE_PT + 2 * LINE_GAP_PT);
    }
    for line in body.iter().flat_map(|l| wrapped(l)) {
        if y < PAGE_MARGIN_PT {
            break;
        }
        put(&line, "F1", BODY_SIZE_PT, y, &mut out);
        y = y.saturating_sub(BODY_SIZE_PT + LINE_GAP_PT);
    }
    out
}

pub fn pdf(deck: &Value) -> Vec<u8> {
    let slides = slides_of(deck);
    // Objects: 1 catalog, 2 pages, 3 and 4 fonts, then a page and its stream each.
    let mut objects: Vec<Vec<u8>> = vec![];
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: String = (0..slides.len()).map(|i| format!("{} 0 R ", 5 + 2 * i)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", slides.len()).into_bytes());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
    for (i, s) in slides.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH_PT} {PAGE_HEIGHT_PT}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                6 + 2 * i
            )
            .into_bytes(),
        );
        let stream = page_stream(s);
        let mut o = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        o.extend(stream);
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deck() -> Value {
        json!({ "type": "deck", "title": "Q3", "slides": [
            { "layout": "title", "title": "Q3 & beyond", "subtitle": "Results" },
            { "layout": "bullets", "title": "Wins", "items": ["One (first)", "Two"] },
        ] })
    }

    #[test]
    fn a_deck_exports_to_a_pptx_with_one_slide_each() {
        let bytes = pptx(&deck());
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"PK\x03\x04"));
        for part in ["[Content_Types].xml", "ppt/presentation.xml", "ppt/slides/slide1.xml", "ppt/slides/slide2.xml"] {
            assert!(text.contains(part), "{part}");
        }
        assert!(text.contains("Q3 &amp; beyond"), "not escaped");
        assert!(!text.contains("slide3.xml"));
    }

    #[test]
    fn a_deck_exports_to_a_pdf_with_one_page_each() {
        let bytes = pdf(&deck());
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("One \\(first\\)"), "not escaped");
        assert!(text.ends_with("%%EOF\n"));
    }
}
