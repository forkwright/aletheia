#![deny(missing_docs)]
//! poiesis-inspect: text extraction from PDF, XLSX, and PPTX documents.
//!
//! Provides functions to read and extract text content from common document
//! formats, allowing agents to inspect their own generated outputs.

mod error;
mod pdf;
mod pptx;
mod xlsx;

pub use error::{InspectError, Result};
pub use pdf::{PdfInspectLimits, read_pdf_file_bounded};

use tracing::instrument;

/// Summary of extracted text and metadata from a PDF document.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSummary {
    /// Number of pages in the PDF.
    pub pages: usize,
    /// Whether `pages` was produced by a successful lopdf parse.
    ///
    /// When `false`, `pages` is set to `1` because lopdf could not count
    /// the pages and no other value is safe to report.
    pub page_count_reliable: bool,
    /// Extracted text snippets from each page.
    pub text_snippets: Vec<String>,
    /// Whether `text_snippets` was truncated to the 100-line storage cap.
    pub truncated: bool,
    /// Total number of non-empty lines in the extracted text.
    ///
    /// When `truncated` is `false`, this equals `text_snippets.len()`.
    pub total_lines: usize,
}

impl PdfSummary {
    pub(crate) fn new(
        pages: usize,
        page_count_reliable: bool,
        text_snippets: Vec<String>,
        truncated: bool,
        total_lines: usize,
    ) -> Self {
        Self {
            pages,
            page_count_reliable,
            text_snippets,
            truncated,
            total_lines,
        }
    }
}

/// Summary of extracted text and metadata from an XLSX workbook.
///
/// This mirrors the structure from sibling crates if they expose it;
/// otherwise it is defined locally here.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct WorkbookSummary {
    /// Sheet names and their extracted text content, in workbook order.
    pub sheets: indexmap::IndexMap<String, String>,
}

/// Summary of extracted text and metadata from a PPTX presentation.
///
/// This mirrors the structure from sibling crates if they expose it;
/// otherwise it is defined locally here.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PresentationSummary {
    /// Slide text content (indexed by slide number).
    pub slides: Vec<String>,
}

/// Extract text from a PDF document.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PDF or if
/// text extraction fails.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pdf(bytes: &[u8]) -> Result<PdfSummary> {
    inspect_pdf_with_limits(bytes, &PdfInspectLimits::default())
}

/// Inspect a PDF using caller-owned resource limits.
///
/// Both ingestion and the agent report tool construct this policy at their
/// boundary, before the PDF bytes are allocated or parsed.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pdf_with_limits(bytes: &[u8], limits: &PdfInspectLimits) -> Result<PdfSummary> {
    contain_pdf_parser(|| pdf::inspect_pdf_impl(bytes, limits))
}

/// Extract a PDF's full text, uncapped.
///
/// WHY(#6751) this exists beside [`inspect_pdf`]: that function summarises, and caps its
/// output at 100 non-empty lines. A consumer that stores the document's content needs
/// all of it -- a capped extraction ingested as if whole records the first hundred lines
/// of a PDF as the document, and the `truncated` flag that would have said so lives on a
/// struct such a caller has no reason to keep.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PDF or if text
/// extraction fails.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    extract_pdf_text_with_limits(bytes, &PdfInspectLimits::default())
}

/// Extract PDF text using caller-owned resource limits.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn extract_pdf_text_with_limits(bytes: &[u8], limits: &PdfInspectLimits) -> Result<String> {
    contain_pdf_parser(|| pdf::extract_pdf_text_impl(bytes, limits))
}

std::thread_local! {
    /// Most recent panic message captured on this thread by the hook
    /// [`ensure_panic_message_capture`] installs.
    static LAST_PANIC_MESSAGE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a process-wide panic hook, once, that records each panic's
/// hook-formatted description (location plus message) into a thread-local
/// before unwinding leaves the panicking frame.
///
/// `catch_unwind`'s `Box<dyn Any + Send>` payload is not reliably a
/// `&str`/`String` to downcast, so the payload is never inspected directly;
/// `PanicHookInfo`'s `Display` impl (the same rendering the default hook
/// prints) is captured instead, since it reads the panic's message
/// independently of the payload's concrete type. The thread-local keeps
/// concurrent callers on different threads from reading each other's
/// message; wrapping (not replacing) the previous hook preserves normal
/// panic output.
fn ensure_panic_message_capture() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            LAST_PANIC_MESSAGE.with(|slot| *slot.borrow_mut() = Some(info.to_string()));
            previous(info);
        }));
    });
}

fn contain_pdf_parser<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    ensure_panic_message_capture();
    LAST_PANIC_MESSAGE.with(|slot| slot.borrow_mut().take());
    // WHY: the panic payload itself is deliberately unused here -- it is not
    // reliably `&str`/`String` to downcast, so the message was already
    // captured into `LAST_PANIC_MESSAGE` by the hook before unwinding
    // reached this `map_err`.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).map_err(|_panic_payload| {
        let detail = LAST_PANIC_MESSAGE
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(|| "PDF parser panicked with no captured message".to_owned());
        InspectError::PdfParserPanicked { detail }
    })?
}

/// Extract text from an XLSX workbook.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid XLSX.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_xlsx(bytes: &[u8]) -> Result<WorkbookSummary> {
    xlsx::inspect_xlsx_impl(bytes)
}

/// Extract text from a PPTX presentation.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PPTX.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pptx(bytes: &[u8]) -> Result<PresentationSummary> {
    pptx::inspect_pptx_impl(bytes)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test fixture construction and assertions"
)]
#[expect(
    clippy::field_reassign_with_default,
    reason = "tests override one policy limit at a time for readability"
)]
mod tests {
    use super::*;

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn text_pdf(text: &str, pages: usize) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, SaveOptions, Stream};

        let mut document = Document::with_version("1.5");
        document.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        let pages_id = document.new_object_id();
        let font_id = document.add_object(lopdf::dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier"
        });
        let resources_id = document.add_object(lopdf::dictionary! {
            "Font" => lopdf::dictionary! { "F1" => font_id }
        });
        let mut page_ids = Vec::with_capacity(pages);
        for _ in 0..pages {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                lopdf::dictionary! {},
                content.encode().expect("encode content"),
            ));
            page_ids.push(document.add_object(lopdf::dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id
            }));
        }
        let page_count = i64::try_from(pages).expect("test page count fits i64");
        let kids = page_ids
            .into_iter()
            .map(Object::Reference)
            .collect::<Vec<_>>();
        document.objects.insert(
            pages_id,
            Object::Dictionary(lopdf::dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => page_count,
                "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(lopdf::dictionary! {
            "Type" => "Catalog", "Pages" => pages_id
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document
            .save_with_options(&mut bytes, SaveOptions::default())
            .expect("save classic-xref PDF");
        bytes
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn object_stream_pdf() -> Vec<u8> {
        use lopdf::{Document, Object, SaveOptions};

        let mut document =
            Document::load_mem(&text_pdf("object stream", 1)).expect("load test PDF");
        for index in 0..8 {
            document.add_object(lopdf::dictionary! {
                "TestObject" => Object::string_literal(format!("object-{index}"))
            });
        }

        let mut bytes = Vec::new();
        document
            .save_with_options(
                &mut bytes,
                SaveOptions::builder()
                    .use_object_streams(true)
                    .use_xref_streams(true)
                    .max_objects_per_stream(1)
                    .compression_level(1)
                    .build(),
            )
            .expect("save object-stream PDF");
        bytes
    }

    fn classic_xref_offset(bytes: &[u8]) -> usize {
        bytes
            .windows(b"\nxref\n".len())
            .rposition(|window| window == b"\nxref\n")
            .map(|position| position + 1)
            .expect("classic xref marker")
    }

    fn cr_only_classic_xref(mut bytes: Vec<u8>) -> Vec<u8> {
        let xref = classic_xref_offset(&bytes);
        for byte in bytes.get_mut(xref..).expect("xref tail in bounds") {
            if *byte == b'\n' {
                *byte = b'\r';
            }
        }
        bytes
    }

    fn crlf_classic_xref(bytes: &[u8]) -> Vec<u8> {
        let xref = classic_xref_offset(bytes);
        let tail = bytes.get(xref..).expect("xref tail in bounds");
        // Each `\n` becomes at most `\r\n` (net +1 byte), so the input length
        // plus the tail length is always a sufficient upper bound -- no need
        // to pre-count newlines to size the buffer exactly.
        let mut converted = Vec::with_capacity(bytes.len() + tail.len());
        converted.extend_from_slice(bytes.get(..xref).expect("xref head in bounds"));
        for byte in tail {
            if *byte == b'\n' {
                if converted.last() == Some(&b' ') {
                    converted.pop();
                }
                converted.extend_from_slice(b"\r\n");
            } else {
                converted.push(*byte);
            }
        }
        converted
    }

    fn classic_xref_with_declared_count_delta(bytes: &[u8], delta: isize) -> Vec<u8> {
        let xref = classic_xref_offset(bytes);
        let header_start = xref + b"xref\n".len();
        let header_tail = bytes
            .get(header_start..)
            .expect("xref subsection header in bounds");
        let header_end = header_tail
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|relative| header_start + relative)
            .expect("xref subsection header end");
        let header = std::str::from_utf8(
            bytes
                .get(header_start..header_end)
                .expect("xref header span in bounds"),
        )
        .expect("ASCII xref header");
        let mut fields = header.split_whitespace();
        let start = fields.next().expect("xref start");
        let count = fields
            .next()
            .expect("xref count")
            .parse::<usize>()
            .expect("numeric xref count");
        let replacement = format!(
            "{start} {}",
            count.checked_add_signed(delta).expect("test count")
        );
        let mut malformed = Vec::with_capacity(bytes.len() + replacement.len());
        malformed.extend_from_slice(
            bytes
                .get(..header_start)
                .expect("xref pre-header in bounds"),
        );
        malformed.extend_from_slice(replacement.as_bytes());
        malformed.extend_from_slice(bytes.get(header_end..).expect("xref post-header in bounds"));
        malformed
    }

    fn raw_classic_pdf(objects: &[(u32, String)]) -> Vec<u8> {
        raw_classic_pdf_with_header(b"%PDF-1.5\n", objects, "")
    }

    fn raw_classic_pdf_with_header(
        header: &[u8],
        objects: &[(u32, String)],
        trailer_extra: &str,
    ) -> Vec<u8> {
        let max_id = objects
            .iter()
            .map(|(id, _)| *id)
            .max()
            .expect("at least one object");
        let mut bytes = header.to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (id, body) in objects {
            offsets.insert(*id, bytes.len());
            bytes.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_offset = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for id in 1..=max_id {
            match offsets.get(&id) {
                Some(offset) => {
                    bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
                }
                None => bytes.extend_from_slice(b"0000000000 00000 f \n"),
            }
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R{trailer_extra} >>\nstartxref\n{xref_offset}\n%%EOF\n",
                max_id + 1
            )
            .as_bytes(),
        );
        bytes
    }

    /// One physical stream payload contains several correctly xref-addressed
    /// object headers. Each nested `/Length` reaches the same final
    /// `endstream`, so an unbounded loader would retain an overlapping source
    /// slice for every distinct object ID.
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn overlapping_direct_stream_pdf(nested_streams: u32) -> Vec<u8> {
        assert!(nested_streams >= 2, "fixture needs overlapping stream IDs");

        let mut payload = Vec::new();
        let mut lengths = Vec::new();
        for id in 4..4 + nested_streams {
            let header = format!("{id} 0 obj\n<< /Length 0000000000 >>\nstream\n");
            let length_start = payload
                .len()
                .checked_add(header.find("0000000000").expect("length placeholder"))
                .expect("fixture offset");
            let content_start = payload
                .len()
                .checked_add(header.len())
                .expect("fixture offset");
            payload.extend_from_slice(header.as_bytes());
            lengths.push((id, length_start, content_start));
        }
        payload.extend(std::iter::repeat_n(b'x', 4096));
        for (id, length_start, content_start) in lengths {
            let length = payload
                .len()
                .checked_sub(content_start)
                .expect("fixture length");
            let encoded = format!("{length:010}");
            let length_end = length_start
                .checked_add(encoded.len())
                .expect("fixture offset");
            payload
                .get_mut(length_start..length_end)
                .expect("length placeholder span in bounds")
                .copy_from_slice(encoded.as_bytes());
            assert!(id > 3, "nested IDs follow the enclosing stream");
        }

        let mut bytes = b"%PDF-1.5\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (id, body) in [
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ] {
            offsets.insert(id, bytes.len());
            bytes.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        offsets.insert(3, bytes.len());
        let outer = format!("3 0 obj\n<< /Length {} >>\nstream\n", payload.len());
        bytes.extend_from_slice(outer.as_bytes());
        let payload_start = bytes.len();
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");

        for id in 4..4 + nested_streams {
            let marker = format!("{id} 0 obj\n");
            let relative = payload
                .windows(marker.len())
                .position(|window| window == marker.as_bytes())
                .expect("nested object marker");
            offsets.insert(
                id,
                payload_start.checked_add(relative).expect("fixture offset"),
            );
        }

        let max_id = 3 + nested_streams;
        let xref_offset = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for id in 1..=max_id {
            let offset = offsets.get(&id).expect("fixture xref offset");
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                max_id + 1
            )
            .as_bytes(),
        );
        bytes
    }

    fn overlapping_direct_string_object_pdf() -> Vec<u8> {
        let mut bytes = b"%PDF-1.5\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n3 0 obj\n(".to_vec();
        let nested_offset = bytes.len();
        bytes.extend_from_slice(b"4 0 obj\n(owned string payload)\nendobj\n)\nendobj\n");
        let xref_offset = bytes.len();
        bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        let root_one = b"%PDF-1.5\n".len();
        let root_two = bytes
            .windows(b"2 0 obj".len())
            .position(|window| window == b"2 0 obj")
            .expect("pages object");
        let outer = bytes
            .windows(b"3 0 obj".len())
            .position(|window| window == b"3 0 obj")
            .expect("outer object");
        for offset in [root_one, root_two, outer, nested_offset] {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        bytes
    }

    /// Object 3 resolves `/Length` through object 4 before object 4's own xref
    /// entry is loaded. Object 4 is a stream rather than an integer, so the
    /// first parse is discarded and the second is retained normally; both
    /// nevertheless allocate an owned stream vector.
    fn reentrant_length_stream_pdf(stream_bytes: usize, length_ref_first: bool) -> Vec<u8> {
        let payload = "x".repeat(stream_bytes);
        let target = (
            4,
            format!("<< /Length {stream_bytes} >>\nstream\n{payload}\nendstream"),
        );
        let referrer = (3, "<< /Length 4 0 R >>\nstream\nx\nendstream".to_owned());
        let mut objects = vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
        ];
        if length_ref_first {
            objects.extend([referrer, target]);
        } else {
            objects.extend([target, referrer]);
        }
        raw_classic_pdf(&objects)
    }

    fn with_broken_startxref(mut bytes: Vec<u8>) -> Vec<u8> {
        let marker = b"startxref\n";
        let start = bytes
            .windows(marker.len())
            .rposition(|window| window == marker)
            .expect("startxref marker")
            + marker.len();
        let end = bytes
            .get(start..)
            .expect("startxref tail in bounds")
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("startxref line")
            + start;
        bytes
            .get_mut(start..end)
            .expect("startxref digits span in bounds")
            .fill(b'9');
        bytes
    }

    fn binary_marked_encrypted_staging_pdf() -> Vec<u8> {
        raw_classic_pdf_with_header(
            b"%PDF-1.5\n%\x80\x81\x82\x83\n",
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
                (3, "<< /Filter /Standard /V 1 >>".to_owned()),
            ],
            " /Encrypt 3 0 R",
        )
    }

    fn direct_object_source_work_just_below_load(bytes: &[u8]) -> usize {
        let object_start = bytes
            .windows(b"1 0 obj".len())
            .position(|window| window == b"1 0 obj")
            .expect("first object");
        let xref_start = bytes
            .windows(b"xref\n".len())
            .position(|window| window == b"xref\n")
            .expect("xref");
        xref_start
            .checked_sub(object_start)
            .and_then(|objects| objects.checked_add(bytes.len() - xref_start))
            .and_then(|required| required.checked_sub(1))
            .expect("non-empty fixture")
    }

    fn assert_direct_object_source_work_is_admitted(body: String) {
        use lopdf::{Document, LoadOptions, SourceWorkBudget};

        let bytes = raw_classic_pdf(&[(1, body)]);
        let error = Document::load_mem_with_options(
            &bytes,
            LoadOptions {
                strict: true,
                source_work_budget: Some(SourceWorkBudget::new(
                    direct_object_source_work_just_below_load(&bytes),
                )),
                ..LoadOptions::default()
            },
        )
        .expect_err("the xref-bounded direct-object work must be admitted before parser ownership");
        assert!(matches!(
            error,
            lopdf::Error::SourceWorkLimitExceeded { .. }
        ));
    }

    /// A real-valued integral `/Length` is accepted only by lopdf's delayed
    /// materialization path, so it exercises the formerly ignored
    /// `read_stream_content` result.
    fn delayed_stream_pdf(stream_bytes: usize) -> Vec<u8> {
        let payload = "x".repeat(stream_bytes);
        raw_classic_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
            (
                3,
                format!("<< /Length {stream_bytes}. >>\nstream\n{payload}\nendstream"),
            ),
        ])
    }

    fn hidden_object_stream_pdf(stream_count: u32) -> Vec<u8> {
        let mut objects = vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
        ];
        for stream_index in 0..stream_count {
            let member_id = 90_u32.checked_add(stream_index).expect("test member id");
            let index = format!("{member_id} 0 ");
            let member = format!("<< /Hidden {stream_index} >>");
            let content = format!("{index}{member}");
            objects.push((
                3 + stream_index,
                format!(
                    "<< /Type /ObjStm /N 1 /First {} /Length {} >>\nstream\n{}\nendstream",
                    index.len(),
                    content.len(),
                    content
                ),
            ));
        }
        raw_classic_pdf(&objects)
    }

    fn mismatched_object_stream_count_pdf() -> Vec<u8> {
        let index = "90 0 ";
        let member = "<< /Hidden true >>";
        let content = format!("{index}{member}");
        raw_classic_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
            (
                3,
                format!(
                    "<< /Type /ObjStm /N 0 /First {} /Length {} >>\nstream\n{}\nendstream",
                    index.len(),
                    content.len(),
                    content
                ),
            ),
        ])
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pdf_with_page_content(content: &[u8]) -> Vec<u8> {
        use lopdf::{Document, Object, Stream};

        let mut document = Document::with_version("1.5");
        let page_tree_id = document.new_object_id();
        let content_id = document.add_object(Stream::new(lopdf::dictionary! {}, content.to_vec()));
        let page_id = document.add_object(lopdf::dictionary! {
            "Type" => "Page", "Parent" => page_tree_id, "Contents" => content_id,
            "Resources" => lopdf::dictionary! {},
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(lopdf::dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(lopdf::dictionary! {
            "Type" => "Catalog", "Pages" => page_tree_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save content PDF");
        bytes
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pdf_with_cmap_fonts(cmap: &[u8], font_count: usize) -> Vec<u8> {
        use lopdf::{Document, Object, Stream};

        let mut document = Document::with_version("1.5");
        let page_tree_id = document.new_object_id();
        let cmap_id = document.add_object(Stream::new(lopdf::dictionary! {}, cmap.to_vec()));
        let mut font_resources = lopdf::Dictionary::new();
        let mut content = b"BT\n".to_vec();
        for index in 0..font_count {
            let name = format!("F{}", index + 1);
            let font_id = document.add_object(lopdf::dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
                "ToUnicode" => cmap_id,
            });
            font_resources.set(name.as_bytes(), font_id);
            content.extend_from_slice(format!("/{name} 12 Tf <41> Tj\n").as_bytes());
        }
        content.extend_from_slice(b"ET\n");
        let content_id = document.add_object(Stream::new(lopdf::dictionary! {}, content));
        let page_id = document.add_object(lopdf::dictionary! {
            "Type" => "Page", "Parent" => page_tree_id, "Contents" => content_id,
            "Resources" => lopdf::dictionary! { "Font" => font_resources },
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(lopdf::dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(lopdf::dictionary! {
            "Type" => "Catalog", "Pages" => page_tree_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save CMap PDF");
        bytes
    }

    fn overflowing_xref_stream_pdf() -> Vec<u8> {
        let mut bytes = b"%PDF-1.5\n".to_vec();
        let xref_offset = bytes.len();
        bytes.extend_from_slice(
            b"1 0 obj\n<< /Type /XRef /Size 1 /W [1 1 1] /Index [4294967295 2] /Length 6 >>\nstream\n",
        );
        bytes.extend_from_slice(&[1, 0, 0, 1, 0, 0]);
        bytes.extend_from_slice(
            format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
        );
        bytes
    }

    const ONE_MAPPING_CMAP: &[u8] = br"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Aletheia-Test def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfchar
<41> <0041>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

    const OVERFLOWING_TARGET_CMAP: &[u8] = br"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Aletheia-Overflow-Test def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfrange
<00> <01> <FFFF>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

    const NAMED_ENTITY_TEXT: &str = r"A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;";
    const DECODED_ENTITY_TEXT: &str = "A & B < C > D 'Q' \"R\" \u{2019}";

    #[test]
    fn inspect_pdf_rejects_invalid_bytes() {
        let malformed = b"not a pdf";
        let result = inspect_pdf(malformed);
        assert!(result.is_err());
    }

    #[test]
    fn hostile_pdf_panic_boundary_returns_a_typed_refusal() {
        let error = contain_pdf_parser::<()>(|| panic!("synthetic parser panic"))
            .expect_err("a parser panic must become a document error");
        let InspectError::PdfParserPanicked { detail } = error else {
            panic!("expected InspectError::PdfParserPanicked, got {error:?}");
        };
        assert!(
            detail.contains("synthetic parser panic"),
            "captured panic detail must contain the panic message, got {detail:?}"
        );
    }

    #[test]
    fn bounded_pdf_extraction_preserves_ordinary_text_and_page_count() {
        let bytes = text_pdf("Hello, bounded PDF!", 2);
        let summary = inspect_pdf(&bytes).expect("inspect ordinary PDF");
        assert_eq!(summary.pages, 2);
        assert!(summary.page_count_reliable);
        assert!(
            summary
                .text_snippets
                .join("\n")
                .contains("Hello, bounded PDF!")
        );
        let extracted = extract_pdf_text(&bytes).expect("extract ordinary PDF");
        assert!(extracted.contains("Hello, bounded PDF!"));
    }

    #[test]
    fn overlapping_xref_streams_are_rejected_as_malformed_before_retention() {
        let hostile = overlapping_direct_stream_pdf(8);
        let limits = PdfInspectLimits::for_input_bytes(hostile.len());
        let error = inspect_pdf_with_limits(&hostile, &limits)
            .expect_err("overlapping xref streams must be rejected as malformed");
        assert!(matches!(error, InspectError::PdfOverlappingObjectSpans));

        let valid = text_pdf("ordinary direct stream", 1);
        lopdf::Document::load_mem_with_options(
            &valid,
            lopdf::LoadOptions {
                retained_bytes_budget: Some(lopdf::RetainedBytesBudget::new(valid.len() * 4)),
                ..lopdf::LoadOptions::default()
            },
        )
        .expect("ordinary direct-stream PDF remains accepted");
    }

    #[test]
    fn direct_string_xref_overlap_is_refused_before_materialization() {
        let error = inspect_pdf_with_limits(
            &overlapping_direct_string_object_pdf(),
            &PdfInspectLimits::default(),
        )
        .expect_err(
            "xref entries inside an owned direct string must not create an overlapping object span",
        );
        assert!(matches!(error, InspectError::PdfOverlappingObjectSpans));
    }

    #[test]
    fn raw_stream_budget_charges_reentrant_same_object_copies() {
        for length_ref_first in [true, false] {
            let bytes = reentrant_length_stream_pdf(4096, length_ref_first);
            for _ in 0..4 {
                let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
                limits.max_retained_stream_bytes = 6144;
                let error = inspect_pdf_with_limits(&bytes, &limits)
                    .expect_err("each reentrant source copy requires its own admission");
                assert!(matches!(
                    error,
                    InspectError::PdfLimitExceeded {
                        limit: "retained stream bytes"
                    }
                ));
            }
        }
    }

    #[test]
    fn direct_name_bytes_are_admitted_before_ownership() {
        assert_direct_object_source_work_is_admitted(format!("/{}", "n".repeat(4096)));
    }

    #[test]
    fn direct_literal_string_bytes_are_admitted_before_ownership() {
        assert_direct_object_source_work_is_admitted(format!("({})", "s".repeat(4096)));
    }

    #[test]
    fn direct_hex_string_bytes_are_admitted_before_ownership() {
        assert_direct_object_source_work_is_admitted(format!("<{}>", "ab".repeat(4096)));
    }

    #[test]
    fn source_work_is_a_distinct_typed_limit() {
        let bytes = text_pdf("source-work", 1);
        let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
        limits.max_source_work_bytes = 1;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("the first parser interval must consume the source-work budget");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "PDF source work"
            }
        ));
    }

    #[test]
    fn lenient_xref_reconstruction_preserves_object_limit_refusal() {
        use lopdf::{Document, LoadOptions};

        let bytes = with_broken_startxref(raw_classic_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
        ]));
        let error = Document::load_mem_with_options(
            &bytes,
            LoadOptions {
                strict: false,
                max_objects: Some(1),
                ..LoadOptions::default()
            },
        )
        .expect_err("lenient reconstruction must retain the caller object limit");
        assert!(matches!(
            error,
            lopdf::Error::ObjectLimitExceeded { limit: 1 }
        ));
    }

    #[test]
    fn lenient_xref_resolution_preserves_retained_byte_refusal() {
        use lopdf::{Document, LoadOptions, RetainedBytesBudget};

        let bytes = with_broken_startxref(delayed_stream_pdf(4096));
        let error = Document::load_mem_with_options(
            &bytes,
            LoadOptions {
                strict: false,
                retained_bytes_budget: Some(RetainedBytesBudget::new(4095)),
                ..LoadOptions::default()
            },
        )
        .expect_err("lenient xref recovery must not launder retained-byte refusal");
        assert!(matches!(
            error,
            lopdf::Error::RetainedBytesLimitExceeded { limit: 4095 }
        ));
    }

    #[test]
    fn raw_stream_budget_propagates_delayed_stream_admission_refusal() {
        let bytes = delayed_stream_pdf(4096);
        let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
        limits.max_retained_stream_bytes = 4095;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("late stream materialization must not swallow an admission refusal");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "retained stream bytes"
            }
        ));
    }

    #[test]
    fn pdf_input_limit_is_checked_before_parser_work() {
        let limits = PdfInspectLimits::for_input_bytes(4);
        let error = inspect_pdf_with_limits(b"%PDF-1.5", &limits).expect_err("input is too large");
        assert!(matches!(error, InspectError::PdfInputTooLarge));
    }

    #[test]
    fn pdf_page_budget_rejects_many_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_pages = 1;
        let error = inspect_pdf_with_limits(&text_pdf("page", 2), &limits)
            .expect_err("page budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "page count"
            }
        ));
    }

    #[test]
    fn pdf_aggregate_text_budget_rejects_many_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_extracted_text_bytes = 8;
        let error = extract_pdf_text_with_limits(&text_pdf("text", 3), &limits)
            .expect_err("aggregate text budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "extracted text"
            }
        ));
    }

    #[test]
    fn pdf_aggregate_load_budget_rejects_many_small_object_streams() {
        let bytes = object_stream_pdf();
        let object_stream_count = bytes
            .windows(b"/ObjStm".len())
            .filter(|window| *window == b"/ObjStm")
            .count();
        assert!(
            object_stream_count >= 2,
            "fixture must contain multiple individually-small object streams"
        );

        let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
        limits.max_decompressed_stream_bytes = 1024;
        limits.max_decompressed_page_bytes = 1024;
        // One reservation is consumed by the xref stream and one by the first
        // object stream. The next otherwise-valid object stream must fail
        // before it can allocate its decoded body.
        limits.max_decompressed_total_bytes = 2 * 1024;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("aggregate eager-load budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "aggregate decompression"
            }
        ));
    }

    #[test]
    fn pdf_object_budget_rejects_before_page_or_text_work() {
        let bytes = object_stream_pdf();
        let mut limits = PdfInspectLimits::default();
        limits.max_objects = 1;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("object budget must be enforced before inspection");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "object count"
            }
        ));
    }

    #[test]
    fn classic_xref_all_supported_line_endings_share_parser_admission() {
        let lf = text_pdf("xref endings", 1);
        for (name, bytes) in [
            ("LF", lf.clone()),
            ("CR", cr_only_classic_xref(lf.clone())),
            ("CRLF", crlf_classic_xref(&lf)),
        ] {
            let summary = inspect_pdf(&bytes).unwrap_or_else(|error| {
                panic!("{name} xref must parse before its budget is lowered: {error}")
            });
            assert_eq!(summary.pages, 1, "{name} xref must reach page inspection");
            let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
            limits.max_objects = 1;
            let outcome = std::panic::catch_unwind(|| inspect_pdf_with_limits(&bytes, &limits));
            let error = outcome
                .unwrap_or_else(|_| panic!("{name} xref must not panic"))
                .expect_err("xref object ceiling must refuse the document");
            assert!(
                matches!(
                    error,
                    InspectError::PdfLimitExceeded {
                        limit: "object count"
                    }
                ),
                "{name} xref rows must obey the same object ceiling: {error}"
            );
        }
    }

    #[test]
    fn classic_xref_declared_count_mismatch_fails_closed() {
        let valid = text_pdf("count mismatch", 1);
        for delta in [-1, 1] {
            let bytes = classic_xref_with_declared_count_delta(&valid, delta);
            let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
            let error = outcome
                .expect("count mismatch must not panic")
                .expect_err("the parser must consume exactly the declared number of xref rows");
            assert!(matches!(error, InspectError::PdfExtractionError { .. }));
        }
    }

    #[test]
    fn hidden_object_stream_members_are_rejected() {
        let bytes = hidden_object_stream_pdf(1);
        let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
        let error = outcome
            .expect("hidden member must not panic")
            .expect_err("an ObjStm member absent from the final xref must not enter the document");
        assert!(matches!(error, InspectError::PdfExtractionError { .. }));
    }

    #[test]
    fn multiple_object_streams_cannot_amplify_the_retained_object_set() {
        let bytes = hidden_object_stream_pdf(2);
        let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
        let error = outcome
            .expect("object-stream amplification must not panic")
            .expect_err(
                "members across ObjStm containers must remain a subset of the bounded final xref",
            );
        assert!(matches!(error, InspectError::PdfExtractionError { .. }));
    }

    #[test]
    fn object_stream_n_mismatch_is_rejected_before_member_allocation() {
        let bytes = mismatched_object_stream_count_pdf();
        let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
        let error = outcome
            .expect("object-stream /N mismatch must not panic")
            .expect_err("an index member beyond /N must be rejected");
        assert!(matches!(error, InspectError::PdfExtractionError { .. }));
    }

    #[test]
    fn aggregate_tounicode_budget_is_shared_across_reused_cmaps() {
        let bytes = pdf_with_cmap_fonts(ONE_MAPPING_CMAP, 2);
        let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
        // Keep this fixture focused on the mapping admission rather than the
        // independently bounded decode budget used to reach each CMap.
        limits.max_decompressed_total_bytes = usize::MAX;
        limits.max_decompressed_stream_bytes = 1024 * 1024;
        limits.max_decompressed_page_bytes = 1024 * 1024;
        limits.max_extracted_text_bytes = 16 * 1024 * 1024;
        limits.max_tounicode_mappings = 1;
        let error = extract_pdf_text_with_limits(&bytes, &limits)
            .expect_err("the second font must charge the shared CMap budget");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "ToUnicode mappings"
            }
        ));
    }

    #[test]
    fn overflowing_tounicode_target_is_a_typed_refusal_not_a_panic() {
        let bytes = pdf_with_cmap_fonts(OVERFLOWING_TARGET_CMAP, 1);
        let outcome = std::panic::catch_unwind(|| extract_pdf_text(&bytes));
        let error = outcome
            .expect("overflowing ToUnicode target must not panic")
            .expect_err("overflowing ToUnicode target must be rejected");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "ToUnicode mappings"
            }
        ));
    }

    #[test]
    fn overflowing_inline_image_dimensions_are_rejected_without_panicking() {
        let bytes = pdf_with_page_content(
            b"BI /W 9223372036854775807 /H 9223372036854775807 /CS /RGB /BPC 64 ID x EI\n",
        );
        let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
        let error = outcome
            .expect("inline-image arithmetic must not panic")
            .expect_err("overflowing inline-image dimensions must refuse the content stream");
        assert!(matches!(error, InspectError::PdfExtractionError { .. }));
    }

    #[test]
    fn overflowing_xref_stream_object_number_is_rejected_without_panicking() {
        let bytes = overflowing_xref_stream_pdf();
        let outcome = std::panic::catch_unwind(|| inspect_pdf(&bytes));
        let error = outcome
            .expect("xref-stream arithmetic must not panic")
            .expect_err("start + index outside u32 must be rejected before xref insertion");
        assert!(matches!(error, InspectError::PdfExtractionError { .. }));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn encrypted_pdf_is_explicitly_unsupported_without_password_probe() {
        use lopdf::{
            Document, EncryptionState, EncryptionVersion, LoadOptions, Object, Permissions,
            RetainedBytesBudget,
        };

        let mut document = Document::load_mem(&text_pdf("secret", 1)).expect("load test PDF");
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal(vec![1_u8; 16]),
                Object::string_literal(vec![2_u8; 16]),
            ]),
        );
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner-password",
            user_password: "user-password",
            key_length: 128,
            permissions: Permissions::all(),
        })
        .expect("create encryption state");
        document.encrypt(&state).expect("encrypt test PDF");
        let mut bytes = Vec::new();
        document
            .save_to(&mut bytes)
            .expect("save encrypted test PDF");

        let error = inspect_pdf(&bytes).expect_err("encrypted PDFs must be unsupported");
        assert!(matches!(error, InspectError::EncryptedPdf));

        let staging_bytes = binary_marked_encrypted_staging_pdf();
        let retained_bytes_budget = RetainedBytesBudget::new(4);
        let error = Document::load_mem_with_options(
            &staging_bytes,
            LoadOptions {
                // The four-byte binary mark is admitted first. The fifth byte
                // is unavailable, so this refusal occurs at raw-object staging.
                retained_bytes_budget: Some(retained_bytes_budget.clone()),
                ..LoadOptions::default()
            },
        )
        .expect_err("encrypted raw-object staging must propagate retained-byte exhaustion");
        assert!(matches!(
            error,
            lopdf::Error::RetainedBytesLimitExceeded { .. }
        ));
        assert_eq!(
            retained_bytes_budget.remaining(),
            0,
            "the binary mark was retained before staging"
        );
    }

    #[test]
    fn pdf_aggregate_page_budget_rejects_many_individually_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_decompressed_stream_bytes = 1024;
        limits.max_decompressed_page_bytes = 1024;
        limits.max_decompressed_total_bytes = 2 * 1024;
        let error = extract_pdf_text_with_limits(&text_pdf("page", 3), &limits)
            .expect_err("aggregate page decode budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "aggregate decompression"
            }
        ));
    }

    #[test]
    fn inspect_xlsx_rejects_invalid_bytes() {
        let invalid = b"not xlsx";
        let result = inspect_xlsx(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn inspect_pptx_rejects_invalid_bytes() {
        let invalid = b"not pptx";
        let result = inspect_pptx(invalid);
        assert!(result.is_err());
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn xlsx_with_shared_string(encoded_text: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c></row>
  </sheetData>
</worksheet>"#;

        let shared_strings = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">
  <si><t>{encoded_text}</t></si>
</sst>"#
        );

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("xl/workbook.xml", options)
            .expect("start workbook.xml");
        zip.write_all(WORKBOOK.as_bytes())
            .expect("write workbook.xml");

        zip.start_file("xl/sharedStrings.xml", options)
            .expect("start sharedStrings.xml");
        zip.write_all(shared_strings.as_bytes())
            .expect("write sharedStrings.xml");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1.xml");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pptx_with_slide_text(encoded_text: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let slide = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p><a:r><a:t>{encoded_text}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#
        );

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("ppt/slides/slide1.xml", options)
            .expect("start slide1.xml");
        zip.write_all(slide.as_bytes()).expect("write slide1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    /// Build a PPTX whose slide parts carry exactly the given numbers, written
    /// to the archive in the order supplied.
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pptx_with_numbered_slides(slides: &[(u32, &str)]) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        for (number, text) in slides {
            let slide = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#
            );
            let name = format!("ppt/slides/slide{number}.xml");
            zip.start_file(&name, options).expect("start slide part");
            zip.write_all(slide.as_bytes()).expect("write slide part");
        }

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_reads_slides_past_a_numbering_gap() {
        // WHY: deleting a slide in PowerPoint leaves the surviving part names
        // unrenumbered, so slide3 can be absent while slide4 exists. Probing
        // upward from slide1 and stopping at the first missing index returned
        // only the first two slides and silently dropped the rest.
        let bytes = pptx_with_numbered_slides(&[(1, "first"), (2, "second"), (4, "fourth")]);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(
            summary.slides,
            vec!["first".to_owned(), "second".to_owned(), "fourth".to_owned()],
            "every slide part must be read, including those past a numbering gap"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_orders_slides_by_number_not_archive_order() {
        // WHY: ZIP entry order is arbitrary, so slide order must come from the
        // part number rather than the order the parts happen to be stored in.
        let bytes = pptx_with_numbered_slides(&[(3, "third"), (1, "first"), (2, "second")]);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(
            summary.slides,
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
            "slides must be ordered by slide number"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_ignores_non_slide_parts() {
        // WHY: sibling prefixes under `ppt/` hold template parts whose names also
        // begin with `slide`, and `_rels` entries repeat the slide part names.
        // Only the exact `ppt/slides/slideN.xml` shape is a slide.
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);
        for name in [
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slides/_rels/slide1.xml.rels",
        ] {
            zip.start_file(name, options).expect("start part");
            zip.write_all(b"<x/>").expect("write part");
        }
        zip.finish().expect("finish zip");
        let bytes = cursor.into_inner();

        let summary = inspect_pptx(&bytes).expect("inspect must succeed");
        assert!(
            summary.slides.is_empty(),
            "non-slide parts must not be counted, got: {:?}",
            summary.slides
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_xlsx_unescapes_xml_character_entities() {
        let bytes = xlsx_with_shared_string(NAMED_ENTITY_TEXT);
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");
        let sheet_text = summary.sheets.get("Sheet1").expect("Sheet1 text");

        assert!(
            sheet_text.contains(DECODED_ENTITY_TEXT),
            "sheet text must contain decoded XML entities, got: {sheet_text}"
        );
        assert!(
            !sheet_text.contains("&amp;"),
            "sheet text must not expose raw XML entities, got: {sheet_text}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_unescapes_xml_character_entities() {
        let bytes = pptx_with_slide_text(NAMED_ENTITY_TEXT);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(summary.slides, vec![DECODED_ENTITY_TEXT.to_string()]);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_xlsx_resolves_shared_strings_and_sheet_order() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Zebra",
                    "columns": [{ "header": "Animal" }],
                    "rows": [["Zebra"]]
                },
                {
                    "name": "Apple",
                    "columns": [{ "header": "Fruit" }],
                    "rows": [["Apple"]]
                },
                {
                    "name": "Mango",
                    "columns": [{ "header": "Tropical" }],
                    "rows": [["Mango"]]
                }
            ]
        });

        let bytes = poiesis_sheet::render_xlsx(&data).expect("render must succeed");
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        let names: Vec<&str> = summary.sheets.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec!["Zebra", "Apple", "Mango"],
            "sheet order must match workbook order"
        );

        for (name, text) in &summary.sheets {
            assert!(
                text.contains(name),
                "sheet '{name}' text must contain its shared-string value, got: {text}"
            );
        }
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn nonsequential_xlsx_fixture() -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/worksheets/sheet3.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#;

        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Alpha" sheetId="1" r:id="rId1"/>
    <sheet name="Beta" sheetId="3" r:id="rId2"/>
  </sheets>
</workbook>"#;

        const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#;

        const SHARED_STRINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>AlphaValue</t></si>
  <si><t>BetaValue</t></si>
</sst>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c></row>
  </sheetData>
</worksheet>"#;

        const SHEET3: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>1</v></c></row>
  </sheetData>
</worksheet>"#;

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("start [Content_Types].xml");
        zip.write_all(CONTENT_TYPES.as_bytes())
            .expect("write [Content_Types].xml");

        zip.start_file("xl/workbook.xml", options)
            .expect("start workbook.xml");
        zip.write_all(WORKBOOK.as_bytes())
            .expect("write workbook.xml");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("start rels");
        zip.write_all(RELS.as_bytes()).expect("write rels");

        zip.start_file("xl/sharedStrings.xml", options)
            .expect("start sharedStrings");
        zip.write_all(SHARED_STRINGS.as_bytes())
            .expect("write sharedStrings");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1");

        zip.start_file("xl/worksheets/sheet3.xml", options)
            .expect("start sheet3");
        zip.write_all(SHEET3.as_bytes()).expect("write sheet3");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_xlsx_resolves_nonsequential_worksheet_paths() {
        let bytes = nonsequential_xlsx_fixture();
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        let names: Vec<&str> = summary.sheets.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec!["Alpha", "Beta"],
            "sheet order must match workbook order"
        );

        let alpha = summary.sheets.get("Alpha").expect("Alpha sheet present");
        assert!(
            alpha.contains("AlphaValue"),
            "Alpha text must contain its shared-string value, got: {alpha}"
        );

        let beta = summary.sheets.get("Beta").expect("Beta sheet present");
        assert!(
            beta.contains("BetaValue"),
            "Beta text must contain its shared-string value, got: {beta}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_counts_real_pages() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let mut content = Vec::new();
        for i in 0..60 {
            content.push(Block::Paragraph(RichText {
                spans: vec![Span::Plain(format!(
                    "This is paragraph {i} with enough words to force line wrapping and page breaks. \
                     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor."
                ))],
            }));
        }
        let doc = Document {
            metadata: Metadata {
                title: "Multi-page".to_owned(),
                author: None,
                created: None,
            },
            content,
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            summary.pages >= 2,
            "expected at least 2 pages for a 60-paragraph doc, got {}",
            summary.pages
        );
        assert!(
            summary.page_count_reliable,
            "page count from a renderable PDF must be marked reliable"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_page_count_reliable_on_valid_pdf() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Single-page".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![Span::Plain("Hello, PDF.".to_owned())],
            })],
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert_eq!(
            summary.pages, 1,
            "single-paragraph doc should report one page"
        );
        assert!(
            summary.page_count_reliable,
            "page count from a valid PDF must be marked reliable"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_reports_truncation_for_long_documents() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let mut content = Vec::new();
        for i in 0..150 {
            content.push(Block::Paragraph(RichText {
                spans: vec![Span::Plain(format!("Line {i}"))],
            }));
        }
        let doc = Document {
            metadata: Metadata {
                title: "Long-document".to_owned(),
                author: None,
                created: None,
            },
            content,
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            summary.truncated,
            "document with more than 100 lines must be marked truncated"
        );
        assert!(
            summary.total_lines > 100,
            "total_lines must report the raw line count, got {}",
            summary.total_lines
        );
        assert_eq!(
            summary.text_snippets.len(),
            100,
            "truncated summary must contain exactly 100 snippets"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_reports_no_truncation_for_short_documents() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Short-document".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![Span::Plain("One line.".to_owned())],
            })],
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            !summary.truncated,
            "short document must not be marked truncated"
        );
        assert_eq!(
            summary.total_lines,
            summary.text_snippets.len(),
            "total_lines must equal snippets when nothing was truncated"
        );
    }
}
