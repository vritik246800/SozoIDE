//! PDF files (F21): a page with nothing but text shows its text, from `pdf-extract`, which can
//! be selected and searched; a page with a picture or a drawing is drawn whole by `hayro` (pure
//! Rust, no system libraries) into a small image that reaches the terminal like any other
//! (Q49, Q50). The pages around the one on screen are made in the background, several at once
//! with `rayon`, so that turning a page shows it at once.

use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
    panic::{self, AssertUnwindSafe},
    path::Path,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
};

use anyhow::{Result, anyhow, bail};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings,
    hayro_interpret::{
        BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
        InterpreterCache, InterpreterSettings, SoftMask, TransformExt, font::GlyphRun,
        interpret_page,
    },
    hayro_syntax::Pdf,
    kurbo::{BezPath, Shape},
    render,
    vello_cpu::color::palette::css::WHITE,
};
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;

/// Pixels across a page drawn as an image: low resolution (Q50), still readable in Sixel, Kitty
/// and iTerm2 terminals, quick to draw and small to send.
const PAGE_PIXELS: f32 = 800.0;
/// A drawing thinner than this, in points, is a rule or an underline: it stays text.
const RULE: f64 = 2.0;
/// Pages made ahead of the one on screen (and one behind it).
pub const BUFFERED: usize = 10;

pub struct PdfDocument {
    pages: Arc<Pages>,
    /// The pages made in the background, and those still being made.
    made: HashMap<usize, PageView>,
    making: HashSet<usize>,
    sender: Sender<(usize, Option<PageView>)>,
    receiver: Receiver<(usize, Option<PageView>)>,
    /// The page on screen, from 0.
    pub page: usize,
}

struct Pages {
    pdf: Pdf,
    /// The text of each page; empty when it could not be read.
    texts: Vec<String>,
}

/// How a page shows: as its lines of text, or as a picture.
#[derive(Clone)]
pub enum PageView {
    Text(Vec<String>),
    Picture(DynamicImage),
}

impl PdfDocument {
    pub fn open(bytes: Vec<u8>) -> Result<Self> {
        let texts = page_texts(&bytes);
        let pdf = Pdf::new(bytes).map_err(|error| anyhow!("Could not read the PDF: {error:?}"))?;
        if pdf.pages().is_empty() {
            bail!("The PDF has no pages");
        }
        let (sender, receiver) = channel();
        Ok(Self {
            pages: Arc::new(Pages { pdf, texts }),
            made: HashMap::new(),
            making: HashSet::new(),
            sender,
            receiver,
            page: 0,
        })
    }

    pub fn pages(&self) -> usize {
        self.pages.pdf.pages().len()
    }

    /// Page `page` (from 0): made in the background already, or made now.
    pub fn view(&mut self, page: usize) -> Option<PageView> {
        self.receive();
        match self.made.get(&page) {
            Some(view) => Some(view.clone()),
            None => self.pages.view(page),
        }
    }

    /// Starts making the pages around the one on screen that are not made yet, several at
    /// once, and forgets those far from it.
    pub fn prefetch(&mut self) {
        self.receive();
        let page = self.page;
        let near = window(page, self.pages());
        self.made.retain(|made, _| near.contains(made));
        let wanted = near
            .filter(|near| *near != page && !self.made.contains_key(near))
            .filter(|near| self.making.insert(*near))
            .collect::<Vec<_>>();
        if wanted.is_empty() {
            return;
        }
        let (pages, sender) = (Arc::clone(&self.pages), self.sender.clone());
        rayon::spawn(move || {
            wanted
                .into_par_iter()
                .for_each_with(sender, |sender, page| {
                    let _ = sender.send((page, pages.view(page)));
                });
        });
    }

    /// Keeps the pages made in the background since the last look.
    fn receive(&mut self) {
        let near = window(self.page, self.pages());
        while let Ok((page, view)) = self.receiver.try_recv() {
            self.making.remove(&page);
            if let Some(view) = view.filter(|_| near.contains(&page)) {
                self.made.insert(page, view);
            }
        }
    }

    #[cfg(test)]
    pub fn made(&self) -> usize {
        self.made.len()
    }

    /// Waits for every page being made in the background.
    #[cfg(test)]
    pub fn wait(&mut self) {
        while !self.making.is_empty() {
            if let Ok((page, view)) = self.receiver.recv() {
                self.making.remove(&page);
                if let Some(view) = view {
                    self.made.insert(page, view);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn render(&self, page: usize) -> Option<DynamicImage> {
        self.pages.render(page)
    }
}

/// The pages kept around `page`: one behind it and `BUFFERED` ahead.
fn window(page: usize, pages: usize) -> RangeInclusive<usize> {
    page.saturating_sub(1)..=(page + BUFFERED).min(pages.saturating_sub(1))
}

impl Pages {
    /// Page `page` (from 0) as text when it has only text, else as a picture.
    fn view(&self, page: usize) -> Option<PageView> {
        let lines = self.texts.get(page).map(|text| text_lines(text));
        match lines {
            Some(lines) if !lines.is_empty() && !self.has_graphics(page)? => {
                Some(PageView::Text(lines))
            }
            _ => self.render(page).map(PageView::Picture),
        }
    }

    /// Whether page `page` draws a picture or a drawing that is not a rule, a background or a
    /// letter.
    fn has_graphics(&self, page: usize) -> Option<bool> {
        let page = self.pdf.pages().get(page)?;
        let (width, height) = page.render_dimensions();
        let transform = page.initial_transform(true).to_kurbo();
        let bounds = hayro::kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let cache = InterpreterCache::new();
        let mut context = Context::new(
            transform,
            bounds,
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        let mut graphics = Graphics {
            page: bounds.area(),
            found: false,
        };
        interpret_page(page, &mut context, &mut graphics);
        Some(graphics.found)
    }

    /// Page `page` (from 0) on a white sheet; the sheet is the page's own colour, not the UI's.
    fn render(&self, page: usize) -> Option<DynamicImage> {
        let page = self.pdf.pages().get(page)?;
        let scale = PAGE_PIXELS / page.render_dimensions().0.max(1.0);
        let pixmap = render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        );
        // An opaque sheet: premultiplied and plain RGBA are the same bytes.
        RgbaImage::from_raw(
            u32::from(pixmap.width()),
            u32::from(pixmap.height()),
            pixmap.data_as_u8_slice().to_vec(),
        )
        .map(DynamicImage::ImageRgba8)
    }
}

/// The lines of a page's text without the blank ones around it, and one blank line at most
/// between paragraphs.
fn text_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if !line.is_empty() || lines.last().is_some_and(|last| !last.is_empty()) {
            lines.push(line.to_owned());
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The text of each page, or none when `pdf-extract` cannot read it. It can panic on a PDF it
/// does not expect: the panic is caught, quietly, and the pages are drawn instead.
fn page_texts(bytes: &[u8]) -> Vec<String> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let texts = panic::catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem_by_pages(bytes)
    }));
    panic::set_hook(hook);
    texts.ok().and_then(Result::ok).unwrap_or_default()
}

/// Watches a page being drawn for a picture or a drawing (Q50).
struct Graphics {
    /// The page's area, in points: a drawing that covers most of it is a background.
    page: f64,
    found: bool,
}

impl<'a> Device<'a> for Graphics {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, _: &DrawMode) {
        // ponytail: any drawing bigger than a rule makes the page a picture, also a table's
        // grid or a box behind code; tell those apart only if text pages come out as pictures.
        let bounds = props.transform.transform_rect_bbox(path.bounding_box());
        if bounds.width() >= RULE && bounds.height() >= RULE && bounds.area() < self.page * 0.9 {
            self.found = true;
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {
        self.found = true;
    }
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// A page that draws a blue square: a picture.
#[cfg(test)]
pub const SQUARE: &str = "0 0 1 rg 20 20 60 60 re f";

/// A PDF of `pages` pages, each a white sheet with a blue square, for the tests.
#[cfg(test)]
pub fn sample(pages: usize) -> Vec<u8> {
    sample_pages(&vec![SQUARE; pages])
}

/// A PDF whose pages draw `contents`, with Helvetica as `/F1`, for the tests.
#[cfg(test)]
pub fn sample_pages(contents: &[&str]) -> Vec<u8> {
    let pages = contents.len();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages)
                .map(|page| format!("{} 0 R", 3 + page * 2))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    let font = "<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>";
    for (page, content) in contents.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources {font} /Contents {} 0 R >>",
            4 + page * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_drawn_as_they_are() {
        let document = PdfDocument::open(sample(3)).unwrap();
        assert_eq!(document.pages(), 3);
        let page = document.render(2).unwrap().to_rgba8();
        assert_eq!(page.width(), 800, "low resolution (Q50)");
        let pixel = |x: u32, y: u32| page.get_pixel(x, y).0;
        // The page is white and the square in its middle blue, as the PDF draws them.
        assert_eq!(pixel(5, 5), [255, 255, 255, 255]);
        assert_eq!(pixel(400, 400), [0, 0, 255, 255]);
        assert!(document.render(3).is_none());
        assert!(PdfDocument::open(b"not a pdf".to_vec()).is_err());
        assert!(is_pdf_path(Path::new("Manual.PDF")));
    }

    #[test]
    fn text_pages_show_their_text_and_pages_with_drawings_a_picture() {
        let text = "BT /F1 10 Tf 5 80 Td (Hello PDF) Tj 0 -40 Td (Second line) Tj ET";
        // A rule under the text stays text; a square next to it makes the page a picture.
        let ruled = format!("{text} 5 30 90 0.5 re f");
        let mixed = format!("{text} {SQUARE}");
        let mut document =
            PdfDocument::open(sample_pages(&[text, &ruled, &mixed, SQUARE])).unwrap();
        for page in [0, 1] {
            let Some(PageView::Text(lines)) = document.view(page) else {
                panic!("page {page} is text");
            };
            assert_eq!(lines.first().map(String::as_str), Some("Hello PDF"));
            assert!(lines.iter().any(|line| line == "Second line"));
            assert!(lines.last().is_some_and(|line| !line.is_empty()));
        }
        assert!(matches!(document.view(2), Some(PageView::Picture(_))));
        assert!(
            matches!(document.view(3), Some(PageView::Picture(_))),
            "no text"
        );
        assert!(document.view(4).is_none());
        assert_eq!(text_lines("\n\n a\n\n\n\nb  \n\n"), [" a", "", "b"]);
    }

    #[test]
    fn the_pages_around_the_one_on_screen_are_made_in_the_background() {
        let mut document = PdfDocument::open(sample(15)).unwrap();
        document.prefetch();
        document.wait();
        assert_eq!(document.made(), BUFFERED, "pages 2 to 11");
        assert!(matches!(document.view(10), Some(PageView::Picture(_))));
        // Far ahead: the pages left behind are forgotten, the new ones made.
        document.page = 12;
        document.prefetch();
        document.wait();
        assert_eq!(document.made(), 3, "pages 12, 14 and 15");
        assert_eq!(window(0, 1), 0..=0);
        assert_eq!(window(5, 100), 4..=15);
    }
}
