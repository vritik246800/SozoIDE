//! PDF files drawn as they are (F21): `hayro` (pure Rust, no system libraries) draws a page into
//! an image, which reaches the terminal like any other image.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf, render, vello_cpu::color::palette::css::WHITE,
};
use image::{DynamicImage, RgbaImage};

/// Pixels across a page: sharp in Sixel, Kitty and iTerm2 terminals and quick to draw.
const PAGE_PIXELS: f32 = 1400.0;

pub struct PdfDocument {
    pdf: Pdf,
    /// The page on screen, from 0.
    pub page: usize,
}

impl PdfDocument {
    pub fn open(bytes: Vec<u8>) -> Result<Self> {
        let pdf = Pdf::new(bytes).map_err(|error| anyhow!("Could not read the PDF: {error:?}"))?;
        if pdf.pages().is_empty() {
            bail!("The PDF has no pages");
        }
        Ok(Self { pdf, page: 0 })
    }

    pub fn pages(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Page `page` (from 0) on a white sheet; the sheet is the page's own colour, not the UI's.
    pub fn render(&self, page: usize) -> Option<DynamicImage> {
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

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// A PDF of `pages` pages, each a white sheet with a blue square, for the tests.
#[cfg(test)]
pub fn sample(pages: usize) -> Vec<u8> {
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
    let content = "0 0 1 rg 20 20 60 60 re f";
    for page in 0..pages {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents {} 0 R >>",
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
        assert_eq!(page.width(), 1400);
        let pixel = |x: u32, y: u32| page.get_pixel(x, y).0;
        // The page is white and the square in its middle blue, as the PDF draws them.
        assert_eq!(pixel(5, 5), [255, 255, 255, 255]);
        assert_eq!(pixel(700, 700), [0, 0, 255, 255]);
        assert!(document.render(3).is_none());
        assert!(PdfDocument::open(b"not a pdf".to_vec()).is_err());
        assert!(is_pdf_path(Path::new("Manual.PDF")));
    }
}
