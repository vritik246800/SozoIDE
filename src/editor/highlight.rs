use std::{path::Path, str::FromStr, sync::LazyLock};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color as ThemeColor, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem,
        ThemeSettings,
    },
    parsing::{SyntaxReference, SyntaxSet},
};

use super::markdown::{is_markdown_path, mermaid_lines};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(ansi_theme);

/// The colours a theme item can use: the 16 ANSI colours and, last, the terminal foreground.
const PALETTE: [Color; 17] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
    Color::Reset,
];

/// TextMate scopes and how they look: one ANSI colour per role, so the terminal theme decides
/// the actual colours (`DESIGN.md` §1.1). The most specific selector wins, as in TextMate.
const SCOPES: &[(&str, Color, FontStyle)] = &[
    (
        "comment, punctuation.definition.comment",
        Color::DarkGray,
        FontStyle::ITALIC,
    ),
    (
        "string, punctuation.definition.string",
        Color::Green,
        FontStyle::empty(),
    ),
    (
        "constant.character.escape, string.regexp",
        Color::Cyan,
        FontStyle::empty(),
    ),
    ("constant.numeric", Color::LightRed, FontStyle::empty()),
    (
        "constant.language, constant.character, constant.other, support.constant, \
         variable.other.constant, entity.name.constant",
        Color::LightRed,
        FontStyle::empty(),
    ),
    (
        "keyword, storage.type, storage.modifier, keyword.declaration",
        Color::Magenta,
        FontStyle::empty(),
    ),
    (
        "keyword.operator, punctuation.accessor",
        Color::Cyan,
        FontStyle::empty(),
    ),
    (
        "entity.name.function, support.function, variable.function, meta.function-call.identifier",
        Color::Blue,
        FontStyle::empty(),
    ),
    (
        "support.macro, entity.name.function.macro, entity.name.macro",
        Color::Cyan,
        FontStyle::empty(),
    ),
    (
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
         entity.name.trait, entity.name.interface, entity.other.inherited-class, support.type, \
         support.class, storage.type.primitive, storage.type.numeric",
        Color::Yellow,
        FontStyle::empty(),
    ),
    (
        "entity.name.namespace, entity.name.module, support.module, entity.name.package",
        Color::LightBlue,
        FontStyle::empty(),
    ),
    ("variable.parameter", Color::Reset, FontStyle::ITALIC),
    ("variable.language", Color::Red, FontStyle::ITALIC),
    ("entity.name.tag", Color::Red, FontStyle::empty()),
    (
        "entity.other.attribute-name",
        Color::Yellow,
        FontStyle::empty(),
    ),
    (
        "meta.attribute, meta.annotation, entity.name.decorator, meta.decorator",
        Color::LightMagenta,
        FontStyle::empty(),
    ),
    (
        "support.type.property-name, entity.name.tag.yaml, meta.mapping.key, \
         variable.other.member, variable.other.property, meta.object-literal.key, \
         entity.name.key, entity.name.tag.toml, variable.other.env, keyword.name.ini",
        Color::Blue,
        FontStyle::empty(),
    ),
    (
        "entity.section, entity.name.table, entity.name.section.group-title",
        Color::Yellow,
        FontStyle::BOLD,
    ),
    ("entity.name.pattern", Color::Green, FontStyle::empty()),
    (
        "markup.heading, entity.name.section",
        Color::Blue,
        FontStyle::BOLD,
    ),
    ("markup.bold", Color::Reset, FontStyle::BOLD),
    ("markup.italic", Color::Reset, FontStyle::ITALIC),
    (
        "markup.underline.link, markup.underline",
        Color::Cyan,
        FontStyle::UNDERLINE,
    ),
    (
        "markup.raw, markup.inline.raw",
        Color::Green,
        FontStyle::empty(),
    ),
    (
        "markup.list, punctuation.definition.list",
        Color::Yellow,
        FontStyle::empty(),
    ),
    ("markup.quote", Color::DarkGray, FontStyle::ITALIC),
    ("markup.inserted", Color::Green, FontStyle::empty()),
    ("markup.deleted, invalid", Color::Red, FontStyle::empty()),
    ("markup.changed", Color::Yellow, FontStyle::empty()),
];

/// Each colour travels through syntect as its index in `PALETTE`, in the red channel.
fn theme_color(color: Color) -> ThemeColor {
    let index = PALETTE
        .iter()
        .position(|entry| *entry == color)
        .unwrap_or(16);
    ThemeColor {
        r: index as u8,
        g: 0,
        b: 0,
        a: 0xFF,
    }
}

fn ansi_theme() -> Theme {
    Theme {
        settings: ThemeSettings {
            foreground: Some(theme_color(Color::Reset)),
            ..ThemeSettings::default()
        },
        scopes: SCOPES
            .iter()
            .filter_map(|(selector, color, font_style)| {
                Some(ThemeItem {
                    scope: ScopeSelectors::from_str(selector).ok()?,
                    style: StyleModifier {
                        foreground: Some(theme_color(*color)),
                        background: None,
                        font_style: Some(*font_style),
                    },
                })
            })
            .collect(),
        ..Theme::default()
    }
}

fn span_style(style: syntect::highlighting::Style) -> Style {
    let mut result = Style::default().fg(PALETTE[usize::from(style.foreground.r).min(16)]);
    for (font, modifier) in [
        (FontStyle::BOLD, Modifier::BOLD),
        (FontStyle::ITALIC, Modifier::ITALIC),
        (FontStyle::UNDERLINE, Modifier::UNDERLINED),
    ] {
        if style.font_style.contains(font) {
            result = result.add_modifier(modifier);
        }
    }
    result
}

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

/// The icon of a file by its name or extension (`devicons`, Nerd Fonts glyphs) in the nearest
/// ANSI colour. Unknown types get the plain document icon instead of the crate's `*`.
pub fn file_icon(path: &Path) -> (char, Color) {
    let mut icon = devicons::FileIcon::from(path);
    if icon == devicons::FileIcon::default() {
        icon = devicons::FileIcon::from(Path::new("file.txt"));
        return (icon.icon, Color::Reset);
    }
    let channel = |range: std::ops::Range<usize>| {
        icon.color
            .get(range)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .unwrap_or(0xC0)
    };
    (
        icon.icon,
        nearest_ansi(channel(1..3), channel(3..5), channel(5..7)),
    )
}

/// The name shown in the editor title: the common languages by name, other files by the syntax
/// that highlights them (`DotENV`, `Git Ignore`, `Dockerfile`…).
pub fn language_name(path: &Path, first_line: &str) -> &'static str {
    let named = match extension(path).as_str() {
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
        "ini" | "cfg" | "conf" => "INI",
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
        "jsp" | "jspf" => "JSP",
        _ => "",
    };
    if !named.is_empty() {
        return named;
    }
    syntax_for(path, first_line).name.as_str()
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The syntax of a file: a `#!` line, its whole name (`Cargo.lock`, `.env`, `Dockerfile`), the
/// configuration files no syntax names, the extension and its aliases, in that order.
fn syntax_for(path: &Path, first_line: &str) -> &'static SyntaxReference {
    let by = |token: &str| SYNTAXES.find_syntax_by_extension(token);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let extension = extension(path);
    first_line
        .starts_with("#!")
        .then(|| SYNTAXES.find_syntax_by_first_line(first_line))
        .flatten()
        .or_else(|| by(name))
        .or_else(|| config_syntax(&name.to_ascii_lowercase(), &extension, first_line).and_then(by))
        .or_else(|| by(alias(&extension)))
        .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text())
}

/// Configuration files that no syntax names, by their name or by how they start: `.env.*`,
/// `*ignore`, `*rc`, `.config` (XML or INI), `.lock` (JSON, TOML or YAML), .NET projects.
fn config_syntax(name: &str, extension: &str, first_line: &str) -> Option<&'static str> {
    let start = first_line.trim_start();
    let object = start.starts_with('{');
    if name == ".env" || name.starts_with(".env.") || extension == "env" {
        return Some(".env");
    }
    if name.starts_with('.') && name.ends_with("ignore") {
        return Some("gitignore");
    }
    if name.starts_with('.') && name.ends_with("rc") && !name[1..].contains('.') {
        return Some(if object {
            "json"
        } else if start.contains('=') {
            "ini"
        } else {
            "yaml"
        });
    }
    match extension {
        "config" => Some(if start.starts_with('<') { "xml" } else { "ini" }),
        "lock" | "resolved" => Some(if object {
            "json"
        } else if start.starts_with('[') || start.contains(" = ") {
            "toml"
        } else {
            "yaml"
        }),
        "jsonc" | "json5" | "webmanifest" | "code-workspace" => Some("json"),
        "csproj" | "fsproj" | "vbproj" | "vcxproj" | "props" | "targets" | "nuspec" | "resx"
        | "wxs" | "xib" | "storyboard" => Some("xml"),
        "service" | "socket" | "timer" | "mount" | "target" | "cnf" => Some("ini"),
        _ => None,
    }
}

/// Extensions highlighted by the syntax of another one.
fn alias(extension: &str) -> &str {
    match extension {
        "pls" | "plsql" | "pks" | "pkb" => "sql",
        "astro" => "html",
        "svg" | "xsd" | "xsl" | "xslt" | "plist" => "xml",
        "cts" | "mts" => "ts",
        "mjs" | "cjs" => "js",
        "jsx" => "tsx",
        "jspf" => "jsp",
        "phtml" => "php",
        "yml" => "yaml",
        "cfg" | "conf" => "ini",
        "rake" | "gemspec" => "rb",
        "kts" => "kt",
        "bash" | "zsh" | "fish" => "sh",
        "cmd" => "bat",
        other => other,
    }
}

pub fn highlight_lines(path: &Path, lines: &[String]) -> Vec<Vec<Span<'static>>> {
    if is_mermaid_path(path) {
        return lines
            .iter()
            .map(|line| highlight_mermaid_line(line))
            .collect();
    }
    let syntax = syntax_for(path, lines.first().map_or("", String::as_str));
    let mermaid = if is_markdown_path(path) {
        mermaid_lines(lines)
    } else {
        Vec::new()
    };
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            // The syntaxes end line comments, and other scopes, at the newline: without it the
            // rest of the file stayed a comment after the first `//` or `#` (E22).
            let line_with_newline = format!("{line}\n");
            let highlighted = highlighter
                .highlight_line(&line_with_newline, &SYNTAXES)
                .map(|regions| {
                    regions
                        .into_iter()
                        .filter_map(|(style, text)| {
                            let text = text.strip_suffix('\n').unwrap_or(text);
                            (!text.is_empty())
                                .then(|| Span::styled(text.to_owned(), span_style(style)))
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
        "sankey",
        "xychart",
        "block",
        "packet",
        "architecture",
        "c4context",
        "c4container",
        "c4component",
        "c4dynamic",
        "c4deployment",
        "zenuml",
        "kanban",
        "radar-beta",
        "treemap-beta",
        "treemap",
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
        "axis",
        "curve",
        "group",
        "service",
        "commit",
        "branch",
        "checkout",
        "merge",
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

/// The ANSI colour closest to an RGB one by hue and brightness, for colours that come from
/// data (file icons); the UI itself never draws RGB.
pub fn nearest_ansi(red: u8, green: u8, blue: u8) -> Color {
    let red = f32::from(red);
    let green = f32::from(green);
    let blue = f32::from(blue);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    if maximum - minimum < 24.0 {
        // Near-white grays are the theme's plain text: the terminal foreground keeps them
        // readable on light themes, where ANSI 15 is white on white.
        return if luminance > 205.0 {
            Color::Reset
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
            assert_eq!(language_name(&path, ""), language);
            let syntax = syntax_for(&path, "");
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
            ("properties", "Java Properties"),
            ("jspf", "JSP"),
            ("mjs", "JavaScript"),
            ("cjs", "JavaScript"),
            ("jsx", "JavaScript"),
            ("rake", "Ruby"),
            ("gemspec", "Ruby"),
            ("kts", "Kotlin"),
            ("bash", "Shell"),
            ("zsh", "Shell"),
            ("fish", "Shell"),
            ("cmd", "Batch"),
        ] {
            let path = std::path::PathBuf::from(format!("sample.{extension}"));
            assert_eq!(language_name(&path, ""), language);
            let syntax = syntax_for(&path, "");
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
    fn files_get_an_icon_for_their_type_in_ansi_colours() {
        let (rust, rust_color) = file_icon(Path::new("main.rs"));
        let (markdown, _) = file_icon(Path::new("README.md"));
        let (unknown, unknown_color) = file_icon(Path::new("notes.zzz"));
        assert_ne!(rust, markdown);
        assert_ne!(unknown, '*');
        assert_eq!(unknown_color, Color::Reset);
        assert!(!matches!(rust_color, Color::Rgb(..) | Color::Indexed(_)));
    }

    #[test]
    fn scopes_get_distinct_ansi_roles() {
        let lines = [
            "// note".to_owned(),
            "struct Point; fn main() { let total = 42; println!(\"hi\"); }".to_owned(),
        ];
        let highlighted = highlight_lines(Path::new("main.rs"), &lines);
        let color_of = |row: usize, text: &str| {
            highlighted[row]
                .iter()
                .find(|span| span.content.contains(text))
                .unwrap_or_else(|| panic!("{text} in {:?}", highlighted[row]))
                .style
        };
        assert_eq!(color_of(0, "note").fg, Some(Color::DarkGray));
        assert!(color_of(0, "note").add_modifier.contains(Modifier::ITALIC));
        assert_eq!(color_of(1, "fn").fg, Some(Color::Magenta));
        assert_eq!(color_of(1, "main").fg, Some(Color::Blue));
        assert_eq!(color_of(1, "Point").fg, Some(Color::Yellow));
        assert_eq!(color_of(1, "42").fg, Some(Color::LightRed));
        assert_eq!(color_of(1, "hi").fg, Some(Color::Green));
        assert_eq!(color_of(1, "println").fg, Some(Color::Cyan));
        assert_eq!(nearest_ansi(0xE3, 0x4C, 0x26), Color::Red);
        assert_eq!(nearest_ansi(0x51, 0x9A, 0xBA), Color::Cyan);
    }

    #[test]
    fn plain_text_uses_the_terminal_foreground() {
        let highlighted = highlight_lines(Path::new("main.rs"), &["let total = count;".into()]);
        let total = highlighted[0]
            .iter()
            .find(|span| span.content.contains("total"))
            .expect("the identifier should be highlighted");
        assert_eq!(total.style.fg, Some(Color::Reset));
        assert!(
            highlighted[0]
                .iter()
                .all(|span| span.style.fg != Some(Color::White))
        );
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

        assert_eq!(language_name(Path::new("diagram.mmd"), ""), "Mermaid");
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

    /// Spans of the last line of `text` in a file called `name`.
    fn last_line(name: &str, text: &str) -> Vec<Span<'static>> {
        let lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        highlight_lines(Path::new(name), &lines).pop().unwrap()
    }

    #[test]
    fn line_comments_end_with_their_line() {
        for (name, text, keyword) in [
            ("Point.java", "// note\nclass Point {}", "class"),
            ("app.js", "// note\nfunction run() {}", "function"),
            ("tool.py", "# note\ndef run():", "def"),
            ("main.c", "// note\nint main() { return 0; }", "return"),
            ("index.php", "<?php\n// note\nfunction run() {}", "function"),
        ] {
            let spans = last_line(name, text);
            assert!(
                spans
                    .iter()
                    .all(|span| span.style.fg != Some(Color::DarkGray)),
                "{name}: the comment ran into the next line: {spans:?}"
            );
            assert!(
                spans
                    .iter()
                    .any(|span| span.content == keyword && span.style.fg == Some(Color::Magenta)),
                "{name}: {spans:?}"
            );
        }
    }

    #[test]
    fn configuration_files_get_their_syntax() {
        for (name, first_line, syntax) in [
            (".env", "API_KEY=1", "DotENV"),
            (".env.production", "API_KEY=1", "DotENV"),
            ("local.env", "API_KEY=1", "DotENV"),
            (
                "Cargo.lock",
                "# This file is automatically @generated by Cargo.",
                "TOML",
            ),
            ("poetry.lock", "[[package]]", "TOML"),
            ("composer.lock", "{", "JSON"),
            ("yarn.lock", "# yarn lockfile v1", "YAML"),
            ("Pipfile.lock", "{", "JSON"),
            ("web.config", "<?xml version=\"1.0\"?>", "XML"),
            ("app.config", "[settings]", "INI"),
            (
                "application.properties",
                "server.port=8080",
                "Java Properties",
            ),
            (
                "vite.config.ts",
                "import { defineConfig } from 'vite'",
                "TypeScript",
            ),
            (
                "astro.config.mjs",
                "import { defineConfig } from 'astro/config';",
                "JavaScript",
            ),
            (".gitignore", "target/", "Git Ignore"),
            (".dockerignore", "target/", "Git Ignore"),
            (".editorconfig", "root = true", "INI"),
            (".prettierrc", "{ \"semi\": false }", "JSON"),
            (".npmrc", "save-exact=true", "INI"),
            ("Dockerfile", "FROM rust:1.97", "Dockerfile"),
            ("Makefile", "all: build", "Makefile"),
            ("go.mod", "module example.com/app", "Gomod"),
            ("requirements.txt", "flask==3.0", "Requirements.txt"),
            (".zshrc", "export PATH", "Bourne Again Shell (bash)"),
            ("nginx.conf", "server {", "nginx"),
            ("App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\">", "XML"),
            (
                "header.jspf",
                "<%@ page pageEncoding=\"UTF-8\" %>",
                "Java Server Page (JSP)",
            ),
            ("deploy", "#!/bin/bash", "Bourne Again Shell (bash)"),
        ] {
            assert_eq!(
                syntax_for(Path::new(name), first_line).name,
                syntax,
                "{name}"
            );
        }
        assert_eq!(language_name(Path::new(".env"), ""), "DotENV");
        assert_eq!(language_name(Path::new("header.jspf"), ""), "JSP");
    }

    #[test]
    fn configuration_keys_values_and_sections_are_coloured() {
        let colour = |name: &str, text: &str, word: &str| {
            last_line(name, text)
                .into_iter()
                .find(|span| span.content.contains(word))
                .unwrap_or_else(|| panic!("{word} in {name}"))
                .style
        };
        assert_eq!(
            colour(".env", "# keys\nAPI_KEY=secret", "API_KEY").fg,
            Some(Color::Blue)
        );
        assert_eq!(
            colour("app.properties", "# keys\nserver.port=8080", "server.port").fg,
            Some(Color::Blue)
        );
        assert_eq!(
            colour("setup.cfg", "; c\n[metadata]", "metadata").fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            colour("Cargo.lock", "# c\nname = \"sozo\"", "name").fg,
            Some(Color::Blue)
        );
        assert_eq!(
            colour("page.jspf", "<% int x = 1; %>", "int").fg,
            Some(Color::Yellow)
        );
    }

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }
}
