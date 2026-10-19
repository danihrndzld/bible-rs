//! YouVersion-style JSON snapshots (as published by github.com/mrk214/bible-data-*): one object
//! with the version's names and copyright, and `books[].chapters[].html`. The chapter HTML marks
//! paragraphs and characters with USFM marker names as classes (`<div class="q">`,
//! `<span class="wj">`), so it is rewritten as USFM and read by the USFM parser.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::model::TranslationBuilder;
use crate::usfm::{self, UsfmParser};
use crate::xml::{self, Tok};

#[derive(Deserialize)]
pub struct Doc {
    #[serde(default)]
    pub local_abbreviation: String,
    #[serde(default)]
    pub local_title: String,
    pub language: Option<Language>,
    pub copyright: Option<Copyright>,
    books: Vec<Book>,
}

#[derive(Deserialize)]
pub struct Language {
    #[serde(default)]
    pub iso_639_1: String,
}

#[derive(Deserialize)]
pub struct Copyright {
    #[serde(default)]
    pub text: String,
}

#[derive(Deserialize)]
struct Book {
    usfm: String,
    #[serde(default)]
    human: String,
    chapters: Vec<Chapter>,
}

#[derive(Deserialize)]
struct Chapter {
    usfm: String,
    #[serde(default)]
    html: String,
}

impl Doc {
    pub fn parse(text: &str) -> Result<Doc> {
        serde_json::from_str(text).context("reading JSON Bible")
    }
}

/// Whether the start of a file looks like a JSON snapshot.
pub fn sniff(head: &str) -> bool {
    head.trim_start_matches('\u{feff}').trim_start().starts_with('{')
        && head.contains("\"books\"")
        && head.contains("\"usfm\"")
}

/// Reads every book of `doc` into `b`.
pub fn read(b: &mut TranslationBuilder, doc: &Doc) -> Result<()> {
    for book in &doc.books {
        let mut src = format!("\\id {}\n\\h {}\n", book.usfm, book.human);
        let mut seen = std::collections::HashSet::new();
        for ch in &book.chapters {
            // Snapshots list the chapter of one-chapter books (Obadiah, Jude…) twice.
            if !seen.insert(&ch.usfm) {
                continue;
            }
            // Introductions ("GEN.INTRO1") have no chapter number and are left out.
            let Some(n) = ch.usfm.rsplit('.').next().and_then(|n| n.parse::<u16>().ok()) else { continue };
            src.push_str(&format!("\n\\c {n}\n"));
            html_to_usfm(&ch.html, &mut src);
        }
        UsfmParser::new(b).parse(&src)?;
    }
    Ok(())
}

/// What closing a `div`/`span` means.
enum Open {
    Nothing,
    /// The end of a skipped subtree (footnotes, cross-references, chapter label).
    Skip,
    /// A verse-number label: its text is the number.
    Label,
    Char(String),
}

fn html_to_usfm(html: &str, out: &mut String) {
    let mut stack: Vec<Open> = Vec::new();
    let mut skipping = 0usize;
    let mut label = false;
    for tok in xml::tokens(html) {
        match tok {
            Tok::Start { name, attrs } if name == "div" || name == "span" => {
                let class = xml::attr(attrs, "class").unwrap_or_default();
                let class = class.split_whitespace().next().unwrap_or_default().to_string();
                let open =
                    if skipping > 0 || class == "note" || class == "r" || (name == "div" && class == "label")
                    {
                        Open::Skip
                    } else if class == "label" {
                        Open::Label
                    } else if name == "div" {
                        if !matches!(class.as_str(), "version" | "book" | "chapter" | "") {
                            out.push_str(&format!("\n\\{class} "));
                        }
                        Open::Nothing
                    } else if usfm::char_style(&class).is_some() {
                        out.push_str(&format!("\\{class} "));
                        Open::Char(class)
                    } else {
                        // `verse`, `content`, `heading`: plain containers.
                        Open::Nothing
                    };
                if matches!(open, Open::Skip) {
                    skipping += 1;
                }
                label = matches!(open, Open::Label);
                stack.push(open);
            }
            Tok::End { name } if name == "div" || name == "span" => match stack.pop() {
                Some(Open::Skip) => skipping -= 1,
                Some(Open::Char(c)) => out.push_str(&format!("\\{c}*")),
                Some(Open::Label) => label = false,
                _ => {}
            },
            Tok::Text(t) if skipping == 0 => {
                if label {
                    out.push_str(&format!("\\v {} ", t.trim()));
                } else {
                    // A backslash would start a USFM marker.
                    out.push_str(&t.replace('\\', ""));
                }
            }
            _ => {}
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::{BookId, ChapterIdx, VerseRef};
    use crate::model::{MarkKind, Style, TranslationMeta};

    // Public-domain KJV wording in the snapshot's format.
    const DOC: &str = r#"{"local_abbreviation":"TST","local_title":"Test Bible","language":{"iso_639_1":"es"},
      "copyright":{"text":"Public domain."},"books":[
      {"usfm":"PSA","human":"Psalms","chapters":[
        {"usfm":"PSA.INTRO1","html":"<div class=\"p\">Intro</div>"},
        {"usfm":"PSA.23","html":"<div class=\"version vid1\"><div class=\"book bkPSA\"><div class=\"chapter ch23\" data-usfm=\"PSA.23\"><div class=\"label\">23</div><div class=\"s\"><span class=\"heading\">The shepherd</span></div><div class=\"d\"><span class=\"content\">A Psalm of David.</span></div><div class=\"q\"><span class=\"verse v1\" data-usfm=\"PSA.23.1\"><span class=\"label\">1</span><span class=\"content\">The L<span class=\"nd\">ord</span> is my shepherd;</span></span></div><div class=\"q\"><span class=\"verse v1\" data-usfm=\"PSA.23.1\"><span class=\"content\">I shall not want.</span><span class=\"note x\"><span class=\"label\">#</span><span class=\" body\">Rev. 7.17.</span></span></span></div><div class=\"b\"></div><div class=\"q\"><span class=\"verse v2\" data-usfm=\"PSA.23.2\"><span class=\"label\">2</span><span class=\"content\">He maketh me to lie down &#225; green pastures.</span></span></div></div></div></div>"}]},
      {"usfm":"JHN","human":"Juan","chapters":[
        {"usfm":"JHN.3","html":"<div class=\"chapter ch3\"><div class=\"label\">3</div><div class=\"r\"><span class=\"content\">Nu. 21.9</span></div><div class=\"p\"><span class=\"verse v16\" data-usfm=\"JHN.3.16\"><span class=\"label\">16</span><span class=\"content\"><span class=\"wj\">For God so loved the world.</span> </span></span></div></div>"},
        {"usfm":"JHN.3","html":"<div class=\"p\"><span class=\"verse v16\"><span class=\"label\">16</span><span class=\"content\">For God so loved the world.</span></span></div>"}]}]}"#;

    #[test]
    fn reads_snapshot_html_as_usfm() {
        assert!(sniff(DOC));
        let doc = Doc::parse(DOC).unwrap();
        assert_eq!((doc.local_abbreviation.as_str(), doc.local_title.as_str()), ("TST", "Test Bible"));
        let mut b = TranslationBuilder::new(TranslationMeta::default());
        read(&mut b, &doc).unwrap();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        let t = b.finish();
        assert_eq!(t.verse_count(), 3);
        let ps = ChapterIdx::new(BookId::PSALMS, 23).unwrap();
        let v1 = t.find(VerseRef::new(BookId::PSALMS, 23, 1)).unwrap();
        // Poetry lines are joined; the second line's mark says where it starts.
        assert_eq!(v1.text, "The Lord is my shepherd;I shall not want.");
        assert!(v1.marks.iter().any(|m| m.kind == MarkKind::Poetry && m.offset == 24));
        assert!(v1.spans.iter().any(|s| s.style == Style::DivineName));
        assert!(t.verses_in(ps).flat_map(|v| v.marks.iter()).any(|m| m.kind == MarkKind::PsalmTitle));
        let v2 = t.find(VerseRef::new(BookId::PSALMS, 23, 2)).unwrap();
        assert_eq!(v2.text, "He maketh me to lie down á green pastures.");
        let jn = t.find(VerseRef::new(BookId(42), 3, 16)).unwrap();
        assert_eq!(jn.text, "For God so loved the world.");
        assert!(jn.spans.iter().any(|s| s.style == Style::WordsOfJesus));
        assert_eq!(t.meta.books[42].short, "Juan");
    }
}
