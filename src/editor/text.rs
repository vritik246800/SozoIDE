//! How a file is written on disk and how its long lines fit the screen (F02, TODO 2.1.7,
//! 2.1.8): the line ends and the encoding it came with, kept when it is saved; lines cut into
//! rows of the editor's width.

use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

/// The encoding a file came with; it is saved the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
}

/// A file read as text: its lines, whether it ends with a newline, whether its lines end with
/// `\r\n`, its encoding, and whether it was text at all (invalid UTF-8 is shown, never saved).
pub struct Decoded {
    pub lines: Vec<String>,
    pub trailing_newline: bool,
    pub crlf: bool,
    pub encoding: Encoding,
    pub valid: bool,
}

/// The text of `bytes`; none when they hold a NUL outside UTF-16 (a binary file).
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let (text, encoding, valid) = match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => {
            let text = String::from_utf8(rest.to_vec());
            let valid = text.is_ok();
            (
                text.unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()),
                Encoding::Utf8Bom,
                valid,
            )
        }
        [0xFF, 0xFE, rest @ ..] => (
            utf16(rest, u16::from_le_bytes),
            Encoding::Utf16Le,
            rest.len() % 2 == 0,
        ),
        [0xFE, 0xFF, rest @ ..] => (
            utf16(rest, u16::from_be_bytes),
            Encoding::Utf16Be,
            rest.len() % 2 == 0,
        ),
        _ => {
            if bytes.contains(&0) {
                return None;
            }
            match std::str::from_utf8(bytes) {
                Ok(text) => (text.to_owned(), Encoding::Utf8, true),
                Err(_) => (
                    String::from_utf8_lossy(bytes).into_owned(),
                    Encoding::Utf8,
                    false,
                ),
            }
        }
    };
    let crlf_count = text.matches("\r\n").count();
    let crlf = crlf_count > 0 && crlf_count * 2 >= text.matches('\n').count();
    let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    Some(Decoded {
        lines,
        trailing_newline: text.ends_with('\n'),
        crlf,
        encoding,
        valid,
    })
}

fn utf16(bytes: &[u8], from: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| from(*pair));
    char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// `text` as the bytes of `encoding`.
pub fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [0xEF, 0xBB, 0xBF].into_iter().chain(text.bytes()).collect(),
        Encoding::Utf16Le => [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        Encoding::Utf16Be => [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect(),
    }
}

/// The width of a character on screen (a tab counts as one, as the editor draws it).
pub fn char_width(character: char) -> usize {
    character.width().unwrap_or(0)
}

/// The columns of `spans` from `from` to `to` (on screen): each character goes with the row it
/// starts on, so a wide one at the edge is never lost.
pub fn slice_spans(spans: &[Span<'static>], from: usize, to: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut column = 0;
    for span in spans {
        let mut text = String::new();
        for character in span.content.chars() {
            let width = char_width(character);
            if column >= from && column < to {
                text.push(character);
            }
            column += width;
        }
        if !text.is_empty() {
            out.push(Span::styled(text, span.style));
        }
        if column >= to {
            break;
        }
    }
    out
}

/// How many rows of `width` columns `line` takes when it wraps.
pub fn wrapped_rows(line: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let total: usize = line.chars().map(char_width).sum();
    total.div_ceil(width).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ends_and_encodings_come_back_as_they_were() {
        let crlf = decode(b"a\r\nb\r\n").unwrap();
        assert_eq!(
            (crlf.lines.clone(), crlf.crlf, crlf.trailing_newline),
            (vec!["a".into(), "b".into()], true, true)
        );
        let mixed = decode(b"a\nb\nc\r\n").unwrap();
        assert!(!mixed.crlf, "most lines end with \\n");
        let bom = decode(b"\xEF\xBB\xBFhi\n").unwrap();
        assert_eq!(
            (bom.encoding, bom.lines[0].as_str()),
            (Encoding::Utf8Bom, "hi")
        );
        assert_eq!(encode("hi\n", Encoding::Utf8Bom), b"\xEF\xBB\xBFhi\n");
        let text = "olá\n";
        for encoding in [
            Encoding::Utf16Le,
            Encoding::Utf16Be,
            Encoding::Utf8,
            Encoding::Utf8Bom,
        ] {
            let decoded = decode(&encode(text, encoding)).unwrap();
            assert_eq!(
                (decoded.encoding, decoded.lines[0].as_str()),
                (encoding, "olá")
            );
            assert!(decoded.valid);
        }
        // A NUL outside UTF-16 is binary; invalid UTF-8 is shown but marked as not text.
        assert!(decode(b"a\0b").is_none());
        assert!(!decode(b"a\xFFb").unwrap().valid);
        assert!(
            !decode(b"\xFF\xFEa").unwrap().valid,
            "an odd byte of UTF-16"
        );
        assert_eq!(decode(b"").unwrap().lines, [""]);
    }

    #[test]
    fn long_lines_cut_into_rows_on_character_boundaries() {
        assert_eq!(wrapped_rows("abcdef", 4), 2);
        assert_eq!(wrapped_rows("", 4), 1);
        assert_eq!(wrapped_rows("abc", 0), 1);
        // A wide character never goes half on one row.
        assert_eq!(wrapped_rows("日本語", 4), 2);
        let spans = vec![Span::raw("ab"), Span::raw("日本c")];
        let text = |spans: Vec<Span<'static>>| {
            spans
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };
        assert_eq!(text(slice_spans(&spans, 0, 4)), "ab日");
        assert_eq!(text(slice_spans(&spans, 4, 8)), "本c");
        // A wide character at the edge goes with the row it starts on.
        assert_eq!(text(slice_spans(&spans, 0, 3)), "ab日");
        assert_eq!(text(slice_spans(&spans, 3, 6)), "本");
    }
}
