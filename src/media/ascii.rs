//! Pictures as text: two pixels in each cell, the upper one in the foreground of `▀` and the
//! lower one in its background, in the picture's own colours (F03). Every terminal draws it,
//! and a frame is only text, so GIFs, videos and models redraw as fast as text does.

use image::{Rgba, RgbaImage, imageops::FilterType};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use rayon::prelude::*;

/// Below this alpha a pixel is the terminal's own background.
const TRANSPARENT: u8 = 128;

/// `picture` fitted in `width` × `height` cells, keeping its proportions, centred.
pub fn render(picture: &RgbaImage, width: u16, height: u16) -> Vec<Line<'static>> {
    let (columns, rows) = (u32::from(width), u32::from(height) * 2);
    if columns == 0 || rows == 0 || picture.width() == 0 || picture.height() == 0 {
        return Vec::new();
    }
    // A cell is about twice as tall as it is wide: half a cell is a square pixel.
    let scale =
        (columns as f32 / picture.width() as f32).min(rows as f32 / picture.height() as f32);
    let fitted_width = ((picture.width() as f32 * scale).round() as u32).clamp(1, columns);
    let fitted_height = ((picture.height() as f32 * scale).round() as u32).clamp(1, rows);
    let fitted = if (fitted_width, fitted_height) == picture.dimensions() {
        picture.clone()
    } else {
        image::imageops::resize(picture, fitted_width, fitted_height, FilterType::Triangle)
    };
    let left = " ".repeat(((columns - fitted_width) / 2) as usize);
    let top = (height as usize - fitted_height.div_ceil(2) as usize) / 2;
    let mut lines = vec![Line::default(); top];
    lines.par_extend((0..fitted_height.div_ceil(2)).into_par_iter().map(|row| {
        let mut spans = vec![Span::raw(left.clone())];
        spans.extend((0..fitted_width).map(|column| {
            let upper = *fitted.get_pixel(column, row * 2);
            let lower = if row * 2 + 1 < fitted_height {
                *fitted.get_pixel(column, row * 2 + 1)
            } else {
                Rgba([0, 0, 0, 0])
            };
            cell(upper, lower)
        }));
        Line::from(spans)
    }));
    lines
}

/// One cell: `▀` with the upper pixel over the lower one, `▄` when only the lower one shows.
fn cell(upper: Rgba<u8>, lower: Rgba<u8>) -> Span<'static> {
    let colour = |Rgba([red, green, blue, alpha]): Rgba<u8>| {
        (alpha >= TRANSPARENT).then_some(Color::Rgb(red, green, blue))
    };
    match (colour(upper), colour(lower)) {
        (Some(upper), lower) => Span::styled(
            "▀",
            Style::default().fg(upper).bg(lower.unwrap_or(Color::Reset)),
        ),
        (None, Some(lower)) => Span::styled("▄", Style::default().fg(lower)),
        (None, None) => Span::raw(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_pixels_go_in_each_cell_centred_in_their_proportions() {
        // 4 × 2 pixels: red over blue on the left, transparent over green on the right.
        let mut picture = RgbaImage::new(4, 2);
        for x in 0..4 {
            let (upper, lower) = if x < 2 {
                ([255, 0, 0, 255], [0, 0, 255, 255])
            } else {
                ([0, 0, 0, 0], [0, 255, 0, 255])
            };
            picture.put_pixel(x, 0, Rgba(upper));
            picture.put_pixel(x, 1, Rgba(lower));
        }
        let lines = render(&picture, 4, 3);
        // 4 × 3 cells hold 4 × 6 pixels: the picture fits as it is, one row of cells, with a
        // row above it.
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.is_empty());
        let row = &lines[1].spans;
        assert_eq!(row.len(), 5, "the margin, then 4 cells");
        assert_eq!(row[1].content, "▀");
        assert_eq!(row[1].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(row[1].style.bg, Some(Color::Rgb(0, 0, 255)));
        assert_eq!(row[4].content, "▄", "transparent over green");
        assert_eq!(row[4].style.fg, Some(Color::Rgb(0, 255, 0)));
        assert_eq!(render(&picture, 8, 4)[1].spans.len(), 9, "it grows to fill");

        // A wide picture in a tall area gets a margin above, none on the side.
        let lines = render(&RgbaImage::new(10, 2), 5, 4);
        assert_eq!(lines.len(), 4 - 2);
        assert!(render(&picture, 0, 3).is_empty());
    }
}
