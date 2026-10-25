use std::{path::Path, sync::LazyLock};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::{easy::HighlightLines, parsing::SyntaxSet};

use super::markdown::{is_markdown_path, mermaid_lines};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEMES: LazyLock<two_face::theme::EmbeddedLazyThemeSet> =
    LazyLock::new(two_face::theme::extra);

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp"
            )
        })
}

pub fn language_name(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "java" => "Java",
        "rs" => "Rust",
        "swift" => "Swift",
        "sql" => "SQL",
        "pls" | "plsql" | "pks" | "pkb" => "PL/SQL",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "cs" => "C#",
        "md" | "markdown" => "Markdown",
        "mmd" | "mermaid" => "Mermaid",
        "html" | "htm" => "HTML",
        "xml" | "xsd" | "xsl" | "xslt" | "plist" => "XML",
        "php" | "phtml" => "PHP",
        "css" => "CSS",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "astro" => "Astro",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "ini" | "cfg" | "conf" | "properties" => "INI",
        "csv" => "CSV",
        "svg" => "SVG",
        "py" | "pyw" => "Python",
        "rb" | "rake" | "gemspec" => "Ruby",
        "kt" | "kts" => "Kotlin",
        "dart" => "Dart",
        "lua" => "Lua",
        "r" => "R",
        "sh" | "bash" | "zsh" | "fish" => "Shell",
        "bat" | "cmd" => "Batch",
        _ => "Plain Text",
    }
}

fn syntax_for_path(path: &Path) -> Option<&'static syntect::parsing::SyntaxReference> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let extension = match extension.as_str() {
        "pls" | "plsql" | "pks" | "pkb" => "sql",
        "astro" => "html",
        "svg" | "xsd" | "xsl" | "xslt" | "plist" => "xml",
        "cts" | "mts" => "ts",
        "phtml" => "php",
        "yml" => "yaml",
        "cfg" | "conf" | "properties" => "ini",
        "rake" | "gemspec" => "rb",
        "kts" => "kt",
        "bash" | "zsh" | "fish" => "sh",
        "cmd" => "bat",
        other => other,
    };
    SYNTAXES
        .find_syntax_by_extension(extension)
        .or_else(|| SYNTAXES.find_syntax_plain_text().into())
}

pub fn highlight_lines(path: &Path, lines: &[String]) -> Vec<Vec<Span<'static>>> {
    if is_mermaid_path(path) {
        return lines
            .iter()
            .map(|line| highlight_mermaid_line(line))
            .collect();
    }
    let Some(syntax) = syntax_for_path(path) else {
        return lines
            .iter()
            .map(|line| vec![Span::raw(line.clone())])
            .collect();
    };
    let mermaid = if is_markdown_path(path) {
        mermaid_lines(lines)
    } else {
        Vec::new()
    };
    let mut highlighter =
        HighlightLines::new(syntax, THEMES.get(two_face::theme::EmbeddedThemeName::Nord));
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let highlighted = highlighter
                .highlight_line(line, &SYNTAXES)
                .map(|regions| {
                    regions
                        .into_iter()
                        .map(|(style, text)| {
                            Span::styled(
                                text.to_owned(),
                                Style::default().fg(ansi_color(style.foreground)),
                            )
                        })
                        .collect()
                })
                .unwrap_or_else(|_| vec![Span::raw(line.clone())]);
            if mermaid.get(index).copied().unwrap_or(false) {
                highlight_mermaid_line(line)
            } else {
                highlighted
            }
        })
        .collect()
}

fn is_mermaid_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "mmd" | "mermaid")
        })
}

fn highlight_mermaid_line(line: &str) -> Vec<Span<'static>> {
    mermaid_tokens(line)
        .into_iter()
        .map(|(style, text)| Span::styled(text.to_owned(), style))
        .collect()
}

fn mermaid_tokens(line: &str) -> Vec<(Style, &str)> {
    const DIAGRAM_TYPES: &[&str] = &[
        "graph",
        "flowchart",
        "sequencediagram",
        "classdiagram",
        "statediagram",
        "statediagram-v2",
        "erdiagram",
        "journey",
        "gantt",
        "pie",
        "gitgraph",
        "mindmap",
        "timeline",
        "quadrantchart",
        "xychart-beta",
        "sankey-beta",
        "block-beta",
        "packet-beta",
        "architecture-beta",
        "requirementdiagram",
    ];
    const KEYWORDS: &[&str] = &[
        "subgraph",
        "end",
        "participant",
        "actor",
        "class",
        "note",
        "loop",
        "alt",
        "else",
        "opt",
        "par",
        "and",
        "critical",
        "option",
        "break",
        "rect",
        "activate",
        "deactivate",
        "title",
        "section",
        "dateformat",
        "axisformat",
        "click",
        "link",
        "style",
        "classdef",
        "linkstyle",
        "direction",
        "acctitle",
        "accdescr",
        "autonumber",
        "create",
        "destroy",
        "return",
        "box",
    ];

    let plain = Style::default().fg(Color::Reset);
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < line.len() {
        let rest = &line[index..];
        if rest.starts_with("%%") {
            tokens.push((Style::default().fg(Color::DarkGray), rest));
            break;
        }
        let character = rest.chars().next().unwrap();
        if character.is_whitespace() {
            let length = rest
                .chars()
                .take_while(|character| character.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            tokens.push((plain, &rest[..length]));
            index += length;
            continue;
        }
        if character == '"' || character == '\'' {
            let quote = character;
            let mut length = quote.len_utf8();
            let mut escaped = false;
            while length < rest.len() {
                let current = rest[length..].chars().next().unwrap();
                length += current.len_utf8();
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == quote {
                    break;
                }
            }
            tokens.push((Style::default().fg(Color::Yellow), &rest[..length]));
            index += length;
            continue;
        }
        if character.is_alphabetic() || character == '_' {
            let mut length = 0;
            for current in rest.chars() {
                let hyphen_in_word = current == '-'
                    && rest[length + 1..]
                        .chars()
                        .next()
                        .is_some_and(|next| next.is_alphanumeric() || next == '_');
                if current.is_alphanumeric() || current == '_' || hyphen_in_word {
                    length += current.len_utf8();
                } else {
                    break;
                }
            }
            let word = &rest[..length];
            let lower = word.to_ascii_lowercase();
            let style = if DIAGRAM_TYPES.contains(&lower.as_str()) {
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else if KEYWORDS.contains(&lower.as_str()) {
                Style::default().fg(Color::Cyan)
            } else {
                plain
            };
            tokens.push((style, word));
            index += length;
            continue;
        }
        if character.is_ascii_digit() {
            let length = rest
                .chars()
                .take_while(|character| {
                    character.is_ascii_digit() || matches!(character, '.' | '_')
                })
                .map(char::len_utf8)
                .sum::<usize>();
            tokens.push((Style::default().fg(Color::Yellow), &rest[..length]));
            index += length;
            continue;
        }
        if "-=.<>~".contains(character) {
            let length = rest
                .chars()
                .take_while(|character| "-=.<>~".contains(*character))
                .map(char::len_utf8)
                .sum::<usize>();
            tokens.push((Style::default().fg(Color::Green), &rest[..length]));
            index += length;
            continue;
        }
        if "[]{}()|;,:".contains(character) {
            tokens.push((
                Style::default().fg(Color::Blue),
                &rest[..character.len_utf8()],
            ));
            index += character.len_utf8();
            continue;
        }
        tokens.push((plain, &rest[..character.len_utf8()]));
        index += character.len_utf8();
    }
    tokens
}

fn ansi_color(color: syntect::highlighting::Color) -> Color {
    let red = f32::from(color.r);
    let green = f32::from(color.g);
    let blue = f32::from(color.b);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    if maximum - minimum < 24.0 {
        return if luminance > 205.0 {
            Color::White
        } else if luminance > 115.0 {
            Color::Gray
        } else {
            Color::DarkGray
        };
    }
    let bright = luminance > 145.0;
    match if maximum == red {
        60.0 * ((green - blue) / (maximum - minimum)).rem_euclid(6.0)
    } else if maximum == green {
        60.0 * ((blue - red) / (maximum - minimum) + 2.0)
    } else {
        60.0 * ((red - green) / (maximum - minimum) + 4.0)
    } as u16
    {
        0..=20 | 331..=360 => {
            if bright {
                Color::LightRed
            } else {
                Color::Red
            }
        }
        21..=65 => {
            if bright {
                Color::LightYellow
            } else {
                Color::Yellow
            }
        }
        66..=170 => {
            if bright {
                Color::LightGreen
            } else {
                Color::Green
            }
        }
        171..=200 => {
            if bright {
                Color::LightCyan
            } else {
                Color::Cyan
            }
        }
        201..=255 => {
            if bright {
                Color::LightBlue
            } else {
                Color::Blue
            }
        }
        _ => {
            if bright {
                Color::LightMagenta
            } else {
                Color::Magenta
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_requested_language() {
        for (extension, language) in [
            ("java", "Java"),
            ("rs", "Rust"),
            ("swift", "Swift"),
            ("sql", "SQL"),
            ("plsql", "PL/SQL"),
            ("go", "Go"),
            ("c", "C"),
            ("cpp", "C++"),
            ("cs", "C#"),
            ("md", "Markdown"),
            ("html", "HTML"),
            ("xml", "XML"),
            ("php", "PHP"),
            ("css", "CSS"),
            ("js", "JavaScript"),
            ("ts", "TypeScript"),
            ("tsx", "TSX"),
            ("astro", "Astro"),
            ("vue", "Vue"),
            ("svelte", "Svelte"),
            ("json", "JSON"),
            ("yaml", "YAML"),
            ("toml", "TOML"),
            ("ini", "INI"),
            ("csv", "CSV"),
            ("svg", "SVG"),
            ("py", "Python"),
            ("rb", "Ruby"),
            ("kt", "Kotlin"),
            ("dart", "Dart"),
            ("lua", "Lua"),
            ("r", "R"),
            ("sh", "Shell"),
            ("bat", "Batch"),
        ] {
            let path = std::path::PathBuf::from(format!("sample.{extension}"));
            assert_eq!(language_name(&path), language);
            let syntax = syntax_for_path(&path).expect("extension should have a syntax");
            assert_ne!(syntax.name, "Plain Text", "missing syntax for {extension}");
        }
    }

    #[test]
    fn recognizes_new_language_aliases() {
        for (extension, language) in [
            ("xsd", "XML"),
            ("xsl", "XML"),
            ("xslt", "XML"),
            ("plist", "XML"),
            ("PHTML", "PHP"),
            ("yml", "YAML"),
            ("cfg", "INI"),
            ("conf", "INI"),
            ("properties", "INI"),
            ("rake", "Ruby"),
            ("gemspec", "Ruby"),
            ("kts", "Kotlin"),
            ("bash", "Shell"),
            ("zsh", "Shell"),
            ("fish", "Shell"),
            ("cmd", "Batch"),
        ] {
            let path = std::path::PathBuf::from(format!("sample.{extension}"));
            assert_eq!(language_name(&path), language);
            let syntax = syntax_for_path(&path).expect("alias should have a syntax");
            assert_ne!(syntax.name, "Plain Text", "missing syntax for {extension}");
        }
    }

    #[test]
    fn highlighting_uses_only_ansi_colors() {
        for (path, line) in [
            ("main.rs", "fn main() {}"),
            ("index.xml", "<root attr=\"value\">text</root>"),
            ("index.php", "<?php echo \"Hello\"; ?>"),
            ("config.yaml", "enabled: true"),
            ("script.sh", "echo \"$HOME\""),
        ] {
            let highlighted = highlight_lines(Path::new(path), &[line.into()]);
            assert!(
                highlighted[0].iter().any(|span| span.style.fg.is_some()),
                "missing highlighting for {path}"
            );
            assert!(
                highlighted[0]
                    .iter()
                    .all(|span| !matches!(span.style.fg, Some(Color::Rgb(..)))),
                "non-ANSI color for {path}"
            );
        }
    }

    #[test]
    fn mermaid_blocks_and_files_use_ansi_highlighting() {
        let lines = vec![
            "```mermaid".to_owned(),
            "flowchart TD".to_owned(),
            "A[Start] --> B{\"Done?\"}".to_owned(),
            "%% comment".to_owned(),
            "```".to_owned(),
        ];
        let highlighted = highlight_lines(Path::new("README.md"), &lines);

        assert_eq!(language_name(Path::new("diagram.mmd")), "Mermaid");
        for (line, spans) in lines.iter().zip(&highlighted) {
            assert_eq!(spans_text(spans), *line);
            assert!(
                spans
                    .iter()
                    .all(|span| !matches!(span.style.fg, Some(Color::Rgb(..))))
            );
        }
        assert!(
            highlighted[1].iter().any(|span| {
                span.content == "flowchart" && span.style.fg == Some(Color::Magenta)
            })
        );
        assert!(
            highlighted[2]
                .iter()
                .any(|span| { span.content == "-->" && span.style.fg == Some(Color::Green) })
        );
        assert!(
            highlighted[2]
                .iter()
                .any(|span| span.style.fg == Some(Color::Yellow))
        );
        assert!(
            highlighted[3]
                .iter()
                .all(|span| span.style.fg == Some(Color::DarkGray))
        );

        let file = highlight_lines(
            Path::new("diagram.mmd"),
            &["sequenceDiagram".to_owned(), "Alice->>Bob: Hi".to_owned()],
        );
        assert_eq!(spans_text(&file[0]), "sequenceDiagram");
        assert!(
            file[0]
                .iter()
                .any(|span| span.style.fg == Some(Color::Magenta))
        );
        let tokens = file[1]
            .iter()
            .map(|span| (span.content.as_ref(), span.style.fg))
            .collect::<Vec<_>>();
        assert!(
            file[1]
                .iter()
                .any(|span| span.content == "->>" && span.style.fg == Some(Color::Green)),
            "{tokens:?}"
        );
    }

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }
}
