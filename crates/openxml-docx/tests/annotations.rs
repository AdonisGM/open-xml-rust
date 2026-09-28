//! Comments, notes, tracked changes, fields, bookmarks and content
//! controls: created, saved, validated against the schemas, reopened and
//! checked; real documents read and edited.

mod common;

use common::{assert_untouched_except, open, part_xml, pn, save_reopen, save_validate_reopen, validate};
use openxml_docx::{
    Checkbox, ContentControl, ContentControlKind, ContentControlType, Document, Error, Field,
    HeaderFooterKind, ListItem, NewComment, NoteKind, NotePosition, NoteProperties, NoteRestart,
    NumberFormat, RevisionInfo, RevisionKind, TableOfContents, TextSpan,
};
use openxml_opc::Package;
use openxml_opc::known::{content_types as ct, rel_types};

fn comment(author: &str, text: &str) -> NewComment {
    NewComment {
        author: author.into(),
        initials: None,
        date: Some("2024-05-06T07:08:09Z".into()),
        text: text.into(),
    }
}

// ----- comments ---------------------------------------------------------------------------

#[test]
fn comments_anchor_to_paragraphs_runs_and_text() {
    let mut doc = Document::new();
    doc.add_paragraph("Whole paragraph comment.");
    let mut p = doc.add_paragraph("");
    p.add_run("first ");
    p.add_run("second ");
    p.add_run("third");
    doc.add_paragraph("The quick brown fox jumps.");

    let a = doc
        .add_comment(0, TextSpan::Paragraph, &comment("Ann Lee", "Line one\nLine two"))
        .unwrap();
    let b = doc
        .add_comment(1, TextSpan::Runs(1, 2), &comment("Bob", "Runs"))
        .unwrap();
    let c = doc
        .add_comment(
            2,
            TextSpan::Text("brown fox"),
            &NewComment {
                initials: Some("CX".into()),
                ..comment("Cy", "Text range")
            },
        )
        .unwrap();
    let reply = doc.reply_to_comment(c, &comment("Ann Lee", "Agreed")).unwrap();
    assert!(matches!(
        doc.add_comment(9, TextSpan::Paragraph, &comment("x", "y")),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        doc.add_comment(2, TextSpan::Text("zebra"), &comment("x", "y")),
        Err(Error::NotFound(_))
    ));
    assert!(doc.reply_to_comment(999, &comment("x", "y")).is_err());

    let back = save_validate_reopen(&mut doc);
    let pkg = back.package();
    let main = pkg.main_part().unwrap();
    assert_eq!(
        pkg.related_part(Some(&main), rel_types::COMMENTS),
        Some(pn("/word/comments.xml"))
    );
    assert_eq!(
        pkg.part(&pn("/word/comments.xml")).unwrap().content_type(),
        ct::WML_COMMENTS
    );
    for style in ["CommentText", "CommentReference"] {
        assert!(back.style_ids().contains(&style), "{style}");
    }
    let comments = back.comments();
    assert_eq!(comments.len(), 4);
    assert_eq!(comments[0].id, a);
    assert_eq!(comments[0].author.as_deref(), Some("Ann Lee"));
    assert_eq!(comments[0].initials.as_deref(), Some("AL"));
    assert_eq!(comments[0].date.as_deref(), Some("2024-05-06T07:08:09Z"));
    assert_eq!(comments[0].text, "Line one\nLine two");
    assert_eq!(
        comments[0].anchored_text.as_deref(),
        Some("Whole paragraph comment.")
    );
    assert_eq!(comments[1].id, b);
    assert_eq!(comments[1].anchored_text.as_deref(), Some("second third"));
    assert_eq!(comments[2].id, c);
    assert_eq!(comments[2].initials.as_deref(), Some("CX"));
    assert_eq!(comments[2].anchored_text.as_deref(), Some("brown fox"));
    assert_eq!(comments[3].id, reply);
    assert_eq!(comments[3].anchored_text.as_deref(), Some("brown fox"));
    // Splitting runs for the text range keeps the text.
    assert_eq!(back.paragraphs()[2].text(), "The quick brown fox jumps.");
    // Reference marks are runs in the CommentReference style.
    let xml = part_xml(pkg, "/word/document.xml");
    assert_eq!(xml.matches("<w:commentReference ").count(), 4);
    assert_eq!(xml.matches(r#"<w:rStyle w:val="CommentReference"/>"#).count(), 4);
    let comments_xml = part_xml(pkg, "/word/comments.xml");
    assert_eq!(comments_xml.matches("<w:annotationRef/>").count(), 4);

    // Removing a comment removes its markup everywhere.
    let mut edit = back;
    edit.remove_comment(b).unwrap();
    assert!(matches!(edit.remove_comment(b), Err(Error::NotFound(_))));
    let back = save_validate_reopen(&mut edit);
    let ids: Vec<i64> = back.comments().iter().map(|c| c.id).collect();
    assert_eq!(ids, [a, c, reply]);
    let xml = part_xml(back.package(), "/word/document.xml");
    assert!(!xml.contains(&format!(r#"w:id="{b}""#)));
    assert_eq!(back.paragraphs()[1].text(), "first second third");
}

#[test]
fn comment_fixture_is_read_and_its_comment_removed() {
    let doc = open("comment.docx");
    let comments = doc.comments();
    assert_eq!(comments.len(), 1);
    let c = &comments[0];
    assert_eq!(c.id, 0);
    assert_eq!(c.author.as_deref(), Some("Unbekannter Autor"));
    assert_eq!(c.date.as_deref(), Some("2019-10-11T05:43:39Z"));
    assert!(c.text.starts_with("This is the first line"), "{}", c.text);
    assert!(c.text.contains("This is the second line"), "{}", c.text);
    // The fixture only has a reference mark, no range.
    assert_eq!(c.anchored_text, None);

    let original = Package::open_path(common::fixture("comment.docx")).unwrap();
    let mut doc = doc;
    doc.remove_comment(0).unwrap();
    let bytes = doc.to_bytes().unwrap();
    let after = Package::from_bytes(&bytes).unwrap();
    assert_untouched_except(&original, &after, &["/word/document.xml", "/word/comments.xml"]);
    let back = Document::from_bytes(&bytes).unwrap();
    assert!(back.comments().is_empty());
    assert!(!part_xml(back.package(), "/word/document.xml").contains("commentReference"));
}

// ----- footnotes and endnotes -------------------------------------------------------------

#[test]
fn footnotes_and_endnotes_with_numbering_options() {
    let mut doc = Document::new();
    let mut p = doc.add_paragraph("Claim one");
    let f1 = p.add_footnote("Source A").unwrap();
    p.add_text(" and two");
    let f2 = p.add_footnote("Source B\nSecond paragraph").unwrap();
    let e1 = doc
        .add_paragraph("Conclusion")
        .add_endnote("See appendix")
        .unwrap();
    assert_eq!((f1, f2, e1), (1, 2, 1));
    doc.set_footnote_properties(&NoteProperties {
        position: Some(NotePosition::BeneathText),
        number_format: Some(NumberFormat::LowerRoman),
        start: Some(3),
        restart: Some(NoteRestart::EachPage),
    })
    .unwrap();
    doc.set_endnote_properties(&NoteProperties {
        position: Some(NotePosition::DocumentEnd),
        number_format: Some(NumberFormat::UpperLetter),
        start: None,
        restart: Some(NoteRestart::Continuous),
    })
    .unwrap();
    assert!(
        doc.set_endnote_properties(&NoteProperties {
            position: Some(NotePosition::PageBottom),
            ..Default::default()
        })
        .is_err()
    );

    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.footnotes().len(), 2);
    assert_eq!(back.footnotes()[0].text, "Source A");
    assert_eq!(back.footnotes()[1].text, "Source B\nSecond paragraph");
    assert_eq!(back.endnotes()[0].text, "See appendix");
    let props = back.footnote_properties();
    assert_eq!(props.position, Some(NotePosition::BeneathText));
    assert_eq!(props.number_format, Some(NumberFormat::LowerRoman));
    assert_eq!(props.start, Some(3));
    assert_eq!(props.restart, Some(NoteRestart::EachPage));
    assert_eq!(
        back.endnote_properties().position,
        Some(NotePosition::DocumentEnd)
    );
    let sect = back.sections()[0].raw().clone();
    assert!(sect.footnote_pr.is_some() && sect.endnote_pr.is_some());

    let pkg = back.package();
    let notes = part_xml(pkg, "/word/footnotes.xml");
    assert!(
        notes.contains(r#"<w:footnote w:type="separator" w:id="-1">"#),
        "{notes}"
    );
    assert!(notes.contains(r#"<w:footnote w:type="continuationSeparator" w:id="0">"#));
    assert!(notes.contains("<w:separator/>") && notes.contains("<w:continuationSeparator/>"));
    assert!(notes.contains("<w:footnoteRef/>"));
    let settings = part_xml(pkg, "/word/settings.xml");
    assert!(
        settings.contains(r#"<w:footnote w:id="-1"/><w:footnote w:id="0"/>"#),
        "{settings}"
    );
    assert!(settings.contains(r#"<w:endnote w:id="-1"/><w:endnote w:id="0"/>"#));
    let body = part_xml(pkg, "/word/document.xml");
    assert_eq!(
        body.matches(r#"<w:rStyle w:val="FootnoteReference"/>"#).count(),
        2
    );
    assert_eq!(body.matches(r#"<w:rStyle w:val="EndnoteReference"/>"#).count(), 1);
    for style in [
        "FootnoteText",
        "FootnoteReference",
        "EndnoteText",
        "EndnoteReference",
    ] {
        assert!(back.style_ids().contains(&style));
    }

    let mut edit = back;
    edit.remove_footnote(f1).unwrap();
    assert!(edit.remove_footnote(f1).is_err());
    // Separators cannot be removed as notes.
    assert!(edit.remove_footnote(-1).is_err());
    edit.remove_endnote(e1).unwrap();
    let back = save_validate_reopen(&mut edit);
    assert_eq!(back.footnotes().iter().map(|n| n.id).collect::<Vec<_>>(), [f2]);
    assert!(back.endnotes().is_empty());
    assert_eq!(back.paragraphs()[0].text(), "Claim one and two");
    let body = part_xml(back.package(), "/word/document.xml");
    assert_eq!(body.matches("<w:footnoteReference ").count(), 1);
}

#[test]
fn note_fixtures_are_read_and_extended() {
    let doc = open("footnotes.docx");
    let notes = doc.footnotes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, 1);
    assert_eq!(notes[0].text, "snoska");
    assert!(doc.text().contains("text so snoskoy"));

    let endnotes = open("endnotes.docx").endnotes();
    assert_eq!(endnotes.len(), 1);
    // Word 2003-era numbering: separators use ids 0 and 1, the note 2.
    assert_eq!(endnotes[0].id, 2);
    assert_eq!(endnotes[0].text, "XXX");

    // Adding a footnote to a real document keeps unrelated parts untouched.
    let original = Package::open_path(common::fixture("footnotes.docx")).unwrap();
    let mut doc = doc;
    let id = doc.paragraph_mut(0).unwrap().add_footnote("Added").unwrap();
    assert_eq!(id, 2);
    let bytes = doc.to_bytes().unwrap();
    let after = Package::from_bytes(&bytes).unwrap();
    assert_untouched_except(
        &original,
        &after,
        &["/word/document.xml", "/word/footnotes.xml", "/word/styles.xml"],
    );
    let back = Document::from_bytes(&bytes).unwrap();
    assert_eq!(back.footnotes().len(), 2);
    assert_eq!(back.footnotes()[1].text, "Added");
    let _ = NoteKind::Footnote;
}

// ----- tracked changes --------------------------------------------------------------------

fn tracked_document() -> Document {
    let mut doc = Document::new();
    doc.set_track_revisions(true).unwrap();
    let ann = RevisionInfo {
        author: "Ann".into(),
        date: Some("2024-01-01T00:00:00Z".into()),
    };
    let bob = RevisionInfo::new("Bob");
    let mut p = doc.add_paragraph("Keep ");
    p.add_insertion("added by Ann ", &ann);
    p.add_deletion("removed by Bob ", &bob);
    p.add_run("end");
    let mut p = doc.add_paragraph("alpha beta gamma");
    p.track_deletion(TextSpan::Text("beta "), &ann).unwrap();
    let mut p = doc.add_paragraph("styled");
    p.track_format_change(0, &bob, |r| {
        r.bold(true);
    })
    .unwrap();
    doc
}

#[test]
fn tracked_changes_are_created_listed_and_validated() {
    let mut doc = tracked_document();
    assert!(
        doc.paragraph_mut(1)
            .unwrap()
            .track_deletion(TextSpan::Text("zzz"), &RevisionInfo::new("x"))
            .is_err()
    );
    let back = save_validate_reopen(&mut doc);
    assert!(back.track_revisions());
    let revs = back.revisions();
    let summary: Vec<(RevisionKind, &str, &str)> = revs
        .iter()
        .map(|r| (r.kind, r.author.as_deref().unwrap(), r.text.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (RevisionKind::Insertion, "Ann", "added by Ann "),
            (RevisionKind::Deletion, "Bob", "removed by Bob "),
            (RevisionKind::Deletion, "Ann", "beta "),
            (RevisionKind::RunFormatting, "Bob", "styled"),
        ]
    );
    assert_eq!(revs[0].date.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert!(revs[1].date.is_some());
    // Deleted text is not text; inserted text is.
    assert_eq!(back.text(), "Keep added by Ann end\nalpha gamma\nstyled");
    let xml = part_xml(back.package(), "/word/document.xml");
    assert!(
        xml.contains("<w:delText xml:space=\"preserve\">removed by Bob </w:delText>"),
        "{xml}"
    );
    assert!(xml.contains("<w:rPrChange "));
    assert!(part_xml(back.package(), "/word/settings.xml").contains("<w:trackRevisions/>"));
}

#[test]
fn accept_and_reject_all_or_per_author() {
    let mut doc = tracked_document();
    assert_eq!(doc.accept_revisions_by("Ann"), 2);
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.text(), "Keep added by Ann end\nalpha gamma\nstyled");
    let left: Vec<String> = back.revisions().into_iter().map(|r| r.author.unwrap()).collect();
    assert_eq!(left, ["Bob", "Bob"]);
    let mut back = back;
    assert_eq!(back.reject_revisions_by("Bob"), 2);
    let back = save_validate_reopen(&mut back);
    assert_eq!(
        back.text(),
        "Keep added by Ann removed by Bob end\nalpha gamma\nstyled"
    );
    assert!(back.revisions().is_empty());
    assert!(!back.paragraphs()[2].runs()[0].is_bold());

    let mut doc = tracked_document();
    assert_eq!(doc.accept_all_revisions(), 4);
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.text(), "Keep added by Ann end\nalpha gamma\nstyled");
    assert!(back.paragraphs()[2].runs()[0].is_bold());
    assert!(back.revisions().is_empty());

    let mut doc = tracked_document();
    assert_eq!(doc.reject_all_revisions(), 4);
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.text(), "Keep removed by Bob end\nalpha beta gamma\nstyled");
    assert!(!back.paragraphs()[2].runs()[0].is_bold());

    let mut doc = tracked_document();
    let n = doc.accept_revisions_where(|r| r.kind == RevisionKind::Deletion);
    assert_eq!(n, 2);
    assert_eq!(
        doc.reject_revisions_where(|r| r.kind == RevisionKind::Insertion),
        1
    );
    assert_eq!(doc.text(), "Keep end\nalpha gamma\nstyled");
    doc.set_track_revisions(false).unwrap();
    assert!(!save_validate_reopen(&mut doc).track_revisions());
}

#[test]
fn delins_fixture_revisions() {
    let doc = open("delins.docx");
    let revs = doc.revisions();
    assert!(revs.iter().all(|r| r.author.as_deref() == Some("pavel")));
    let kinds = |k: RevisionKind| revs.iter().filter(|r| r.kind == k).count();
    assert!(kinds(RevisionKind::Insertion) >= 2, "{revs:#?}");
    assert!(kinds(RevisionKind::Deletion) >= 2);
    assert!(kinds(RevisionKind::ParagraphDeletion) >= 2);
    assert!(kinds(RevisionKind::RunFormatting) >= 1);
    let deleted: String = revs
        .iter()
        .filter(|r| r.kind == RevisionKind::Deletion)
        .map(|r| r.text.as_str())
        .collect();
    assert!(deleted.contains("Tika Waylan"), "{deleted}");
    assert!(!doc.text().contains("Tika Waylan"));

    let mut accepted = open("delins.docx");
    let n = accepted.accept_all_revisions();
    assert_eq!(n, revs.len());
    let back = save_validate_reopen(&mut accepted);
    assert!(back.revisions().is_empty());
    assert!(!back.text().contains("Tika Waylan"));
    assert!(back.text().contains("March 2009: Apache Tika Release"));
    // The deleted paragraph marks merged their paragraphs.
    assert!(back.paragraphs().len() < doc.paragraphs().len());

    let mut rejected = open("delins.docx");
    rejected.reject_all_revisions();
    let back = save_validate_reopen(&mut rejected);
    assert!(back.revisions().is_empty());
    assert!(
        back.text()
            .contains("Tika Waylan, a major character in the DragonLance series")
    );
    assert!(!back.text().contains("March 2009: Apache Tika Release"));
}

// ----- fields -----------------------------------------------------------------------------

#[test]
fn simple_and_complex_fields_with_cached_results() {
    let mut doc = Document::new();
    let mut footer = doc.set_footer("Page ").unwrap();
    footer.add_field(&Field::Page, "1");
    footer.add_text(" of ");
    footer.add_simple_field(&Field::NumPages, "3");
    let mut p = doc.add_paragraph("Today: ");
    p.add_simple_field(&Field::Date(Some("dd/MM/yyyy".into())), "31/01/2024");
    p.add_text(" at ");
    p.add_field(&Field::Time(Some("HH:mm".into())), "09:30");
    doc.add_paragraph("Author: ")
        .add_field(&Field::Code("AUTHOR \\* Upper".into()), "ANN");
    doc.set_update_fields_on_open(true).unwrap();

    let back = save_validate_reopen(&mut doc);
    let fields = back.fields();
    let pairs: Vec<(&str, &str)> = fields
        .iter()
        .map(|f| (f.instruction.as_str(), f.result.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            (r#"DATE \@ "dd/MM/yyyy""#, "31/01/2024"),
            (r#"TIME \@ "HH:mm""#, "09:30"),
            (r#"AUTHOR \* Upper"#, "ANN"),
        ]
    );
    // Cached results are part of the text; instructions are not.
    assert_eq!(back.paragraphs()[0].text(), "Today: 31/01/2024 at 09:30");
    let footer = back.footer().unwrap();
    assert_eq!(footer.text(), "Page 1 of 3");
    let footer_fields: Vec<String> = footer.fields().into_iter().map(|f| f.instruction).collect();
    assert_eq!(footer_fields, ["PAGE", "NUMPAGES"]);
    assert!(part_xml(back.package(), "/word/settings.xml").contains("<w:updateFields/>"));
    let _ = HeaderFooterKind::Footer;
}

#[test]
fn table_of_contents_links_to_headings() {
    let mut doc = Document::new();
    doc.add_heading("Report", 0).unwrap();
    doc.add_heading("Introduction", 1).unwrap();
    doc.add_paragraph("Text.");
    doc.add_heading("Details", 2).unwrap();
    doc.add_heading("Deep detail", 4).unwrap();
    doc.add_heading("Summary", 1).unwrap();
    doc.insert_table_of_contents(1, &TableOfContents::default())
        .unwrap();

    let back = save_validate_reopen(&mut doc);
    let toc = back
        .fields()
        .into_iter()
        .find(|f| f.instruction.starts_with("TOC"))
        .unwrap();
    assert_eq!(toc.instruction, r#"TOC \o "1-3" \h \z \u"#);
    assert_eq!(toc.result, "Introduction\nDetails\nSummary");
    let paragraphs = back.paragraphs();
    assert_eq!(paragraphs[1].text(), "Contents");
    assert_eq!(paragraphs[1].style_id(), Some("TOCHeading"));
    let styles: Vec<_> = paragraphs[2..5].iter().map(|p| p.style_id().unwrap()).collect();
    assert_eq!(styles, ["TOC1", "TOC2", "TOC1"]);
    // Entries link to bookmarks around the headings.
    let anchor = paragraphs[2].hyperlinks()[0].anchor.clone().unwrap();
    let bookmark = back.bookmarks().into_iter().find(|b| b.name == anchor).unwrap();
    assert_eq!(bookmark.text, "Introduction");
    assert!(back.settings().unwrap().update_fields.is_some());

    let mut empty = Document::new();
    empty
        .add_table_of_contents(&TableOfContents {
            title: None,
            hyperlinks: false,
            ..Default::default()
        })
        .unwrap();
    let back = save_validate_reopen(&mut empty);
    assert_eq!(back.fields()[0].result, "No table of contents entries found.");
    assert_eq!(back.fields()[0].instruction, r#"TOC \o "1-3" \z \u"#);
}

#[test]
fn field_codes_fixture() {
    let doc = open("FieldCodes.docx");
    let fields = doc.fields();
    let author = fields
        .iter()
        .find(|f| f.instruction.starts_with("AUTHOR"))
        .unwrap();
    assert_eq!(author.instruction, r#"AUTHOR  \* Upper  \* MERGEFORMAT"#);
    assert_eq!(author.result, "ANTONI");
    let created = fields
        .iter()
        .find(|f| f.instruction.starts_with("CREATEDATE"))
        .unwrap();
    assert_eq!(created.result, "16 June 2010");
}

// ----- bookmarks --------------------------------------------------------------------------

#[test]
fn bookmarks_are_added_listed_linked_and_removed() {
    let mut doc = Document::new();
    doc.add_paragraph("Chapter one starts here.");
    doc.add_paragraph("Some text.");
    doc.add_bookmark(0, TextSpan::Text("Chapter one"), "chapter1")
        .unwrap();
    doc.add_bookmark(1, TextSpan::Paragraph, "_hidden").unwrap();
    assert!(doc.add_bookmark(1, TextSpan::Paragraph, "chapter1").is_err());
    assert!(doc.add_bookmark(1, TextSpan::Paragraph, "has space").is_err());
    doc.add_paragraph("Go to ")
        .add_internal_hyperlink("chapter one", "chapter1")
        .unwrap();

    let back = save_validate_reopen(&mut doc);
    let marks = back.bookmarks();
    assert_eq!(marks.len(), 2);
    assert_eq!(
        (marks[0].name.as_str(), marks[0].text.as_str()),
        ("chapter1", "Chapter one")
    );
    assert_eq!(
        (marks[1].name.as_str(), marks[1].text.as_str()),
        ("_hidden", "Some text.")
    );
    assert_eq!(
        back.paragraphs()[2].hyperlinks()[0].anchor.as_deref(),
        Some("chapter1")
    );

    let mut edit = back;
    edit.remove_bookmark("chapter1").unwrap();
    assert!(edit.remove_bookmark("chapter1").is_err());
    let back = save_validate_reopen(&mut edit);
    assert_eq!(back.bookmarks().len(), 1);
    assert_eq!(
        back.text(),
        "Chapter one starts here.\nSome text.\nGo to chapter one"
    );
}

#[test]
fn bookmarks_fixture() {
    let doc = open("bookmarks.docx");
    let marks = doc.bookmarks();
    let pairs: Vec<(&str, &str)> = marks.iter().map(|b| (b.name.as_str(), b.text.as_str())).collect();
    assert_eq!(
        pairs[..2],
        [
            ("poi", "Sample Word Document"),
            (
                "xwpf",
                "This is a sample Microsoft Word Document having bookmarks."
            )
        ]
    );
}

// ----- content controls and check boxes ---------------------------------------------------

#[test]
fn content_controls_are_created_read_and_set() {
    let mut doc = Document::new();
    let name = ContentControl::new(ContentControlKind::PlainText { multi_line: false }, "name");
    let fruit = ContentControl::new(
        ContentControlKind::DropDown(vec![
            ListItem::new("Apple", "apple"),
            ListItem::new("Pear", "pear"),
        ]),
        "fruit",
    );
    let city = ContentControl::new(
        ContentControlKind::ComboBox(vec![ListItem::new("Paris", "paris")]),
        "city",
    );
    let due = ContentControl::new(
        ContentControlKind::Date {
            format: "dd MMMM yyyy".into(),
        },
        "due",
    );
    let body = ContentControl::new(ContentControlKind::RichText, "body");
    let mut p = doc.add_paragraph("Name: ");
    p.add_content_control(&name, "").unwrap();
    let mut p = doc.add_paragraph("Fruit: ");
    p.add_content_control(&fruit, "apple").unwrap();
    assert!(p.add_content_control(&fruit, "banana").is_err());
    doc.add_paragraph("City: ")
        .add_content_control(&city, "Hanoi")
        .unwrap();
    doc.add_paragraph("Due: ")
        .add_content_control(&due, "2024-02-29")
        .unwrap();
    doc.add_block_content_control(&body, "Rich block").unwrap();

    let back = save_validate_reopen(&mut doc);
    let controls = back.content_controls();
    assert_eq!(controls.len(), 5);
    assert_eq!(controls[0].kind, ContentControlType::PlainText);
    assert!(controls[0].showing_placeholder);
    assert_eq!(controls[0].text, "Click here to enter text.");
    assert_eq!(controls[1].kind, ContentControlType::DropDown);
    assert_eq!(
        (controls[1].text.as_str(), controls[1].value.as_deref()),
        ("Apple", Some("apple"))
    );
    assert_eq!(controls[1].items.len(), 2);
    assert_eq!(controls[2].kind, ContentControlType::ComboBox);
    assert_eq!(controls[2].text, "Hanoi");
    assert_eq!(controls[3].kind, ContentControlType::Date);
    assert_eq!(controls[3].text, "29 February 2024");
    assert_eq!(controls[3].value.as_deref(), Some("2024-02-29T00:00:00Z"));
    assert_eq!(controls[4].kind, ContentControlType::RichText);
    assert!(controls[4].block);
    assert_eq!(controls[4].tag.as_deref(), Some("body"));
    assert_eq!(controls[4].title.as_deref(), Some("body"));
    assert!(controls.iter().all(|c| c.id.is_some()));

    let mut edit = back;
    edit.set_content_control_value("name", "Ann").unwrap();
    edit.set_content_control_value("fruit", "Pear").unwrap();
    assert!(edit.set_content_control_value("fruit", "banana").is_err());
    edit.set_content_control_value("city", "paris").unwrap();
    edit.set_content_control_value("due", "2025-07-04").unwrap();
    assert!(edit.set_content_control_value("due", "soon").is_err());
    edit.set_content_control_value("body", "Replaced").unwrap();
    assert!(matches!(
        edit.set_content_control_value("none", "x"),
        Err(Error::NotFound(_))
    ));
    let back = save_validate_reopen(&mut edit);
    let controls = back.content_controls();
    assert_eq!(controls[0].text, "Ann");
    assert!(!controls[0].showing_placeholder);
    assert_eq!(
        (controls[1].text.as_str(), controls[1].value.as_deref()),
        ("Pear", Some("pear"))
    );
    assert_eq!(controls[2].text, "Paris");
    assert_eq!(controls[3].text, "04 July 2025");
    assert_eq!(controls[4].text, "Replaced");
    assert_eq!(
        back.text(),
        "Name: Ann\nFruit: Pear\nCity: Paris\nDue: 04 July 2025\nReplaced"
    );
}

#[test]
fn legacy_form_checkboxes() {
    let mut doc = Document::new();
    doc.add_paragraph("I agree ")
        .add_checkbox("Agree", false)
        .unwrap();
    doc.add_paragraph("Newsletter ")
        .add_checkbox("News", true)
        .unwrap();
    doc.set_checkbox("Agree", true).unwrap();
    let back = save_validate_reopen(&mut doc);
    assert_eq!(
        back.checkboxes(),
        [
            Checkbox {
                name: "Agree".into(),
                checked: true
            },
            Checkbox {
                name: "News".into(),
                checked: true
            }
        ]
    );
    let xml = part_xml(back.package(), "/word/document.xml");
    assert!(xml.contains("FORMCHECKBOX"));
    assert!(
        xml.contains("<w:checkBox><w:sizeAuto/><w:default w:val=\"false\"/><w:checked/></w:checkBox>"),
        "{xml}"
    );

    let fixture = open("checkboxes.docx");
    let boxes = fixture.checkboxes();
    assert_eq!(boxes.len(), 9);
    assert_eq!(
        boxes[0],
        Checkbox {
            name: "Check1".into(),
            checked: false
        }
    );
    assert_eq!(
        boxes[1],
        Checkbox {
            name: "Check2".into(),
            checked: true
        }
    );
    assert_eq!(boxes.iter().filter(|b| b.checked).count(), 5);
    let mut fixture = fixture;
    assert_eq!(fixture.set_checkbox("Check1", true).unwrap(), 2);
    let back = save_reopen(&mut fixture);
    assert!(back.checkboxes()[0].checked);
}

#[test]
fn untouched_parts_stay_identical_after_annotations() {
    let original = Package::open_path(common::fixture("Headers.docx")).unwrap();
    let mut doc = open("Headers.docx");
    doc.add_comment(0, TextSpan::Paragraph, &comment("Ann", "Check"))
        .unwrap();
    doc.add_bookmark(0, TextSpan::Paragraph, "start").unwrap();
    let bytes = doc.to_bytes().unwrap();
    let after = Package::from_bytes(&bytes).unwrap();
    assert_untouched_except(&original, &after, &["/word/document.xml", "/word/styles.xml"]);
    for part in [
        "/word/header1.xml",
        "/word/header2.xml",
        "/word/header3.xml",
        "/word/footnotes.xml",
        "/word/settings.xml",
    ] {
        assert_eq!(
            original.part(&pn(part)).unwrap().data(),
            after.part(&pn(part)).unwrap().data()
        );
    }
    let new_comments = after.part(&pn("/word/comments.xml")).unwrap();
    assert_eq!(new_comments.content_type(), ct::WML_COMMENTS);
    validate(
        &Package::from_bytes(&{
            let mut fresh = Document::from_bytes(&bytes).unwrap();
            fresh.to_bytes().unwrap()
        })
        .unwrap(),
    );
}
