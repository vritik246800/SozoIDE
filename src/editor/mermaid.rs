//! Mermaid diagrams drawn as text: `mermaid-text` draws most kinds; the ones it refuses are
//! rewritten into kinds it draws (C4, ZenUML, `Class : member`, front matter) or drawn here
//! (Kanban, Radar, Treemap). Plain text out: the editor colours the box characters.

/// The diagram of a Mermaid block, `None` when it cannot be drawn (the block stays code).
pub fn render(source: &str) -> Option<Vec<String>> {
    let (title, body) = front_matter(source);
    let kind = kind(&body)?;
    let (heading, mut lines) = match kind.as_str() {
        "kanban" => (None, kanban(&body)),
        "radar-beta" | "radar" => radar(&body),
        "treemap-beta" | "treemap" => (None, treemap(&body)),
        "zenuml" => {
            let (heading, source) = zenuml(&body);
            (heading, drawn(&source)?)
        }
        "classdiagram" | "classdiagram-v2" => (None, drawn(&class_members(&body))?),
        kind if kind.starts_with("c4") => {
            let (heading, source) = c4(&body);
            (heading, drawn(&source)?)
        }
        _ => (None, drawn(&body)?),
    };
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return None;
    }
    if let Some(title) = title.or(heading) {
        lines.splice(0..0, [title, String::new()]);
    }
    Some(lines)
}

fn drawn(source: &str) -> Option<Vec<String>> {
    let drawn = mermaid_text::render(source).ok()?;
    Some(
        drawn
            .lines()
            .map(|line| line.trim_end().to_owned())
            .collect(),
    )
}

/// The `title` of a `---` front matter block and the diagram after it.
fn front_matter(source: &str) -> (Option<String>, String) {
    let mut lines = source.lines().skip_while(|line| line.trim().is_empty());
    if lines.next().map(str::trim) != Some("---") {
        return (None, source.to_owned());
    }
    let mut title = None;
    for line in lines.by_ref() {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("title:") {
            title = Some(unquote(value.trim()).to_owned());
        }
    }
    (title, lines.collect::<Vec<_>>().join("\n"))
}

/// The diagram keyword, lower case: the first word that is not a comment or a directive.
fn kind(source: &str) -> Option<String> {
    source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_ascii_lowercase)
}

/// The lines of the diagram after its keyword, trimmed, without blanks and `%%` comments.
fn statements(source: &str) -> impl Iterator<Item = &str> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .skip(1)
}

fn unquote(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(text)
}

/// Text that goes inside a node or an edge label: no characters the flowchart syntax reads.
fn label(text: &str) -> String {
    unquote(text)
        .chars()
        .map(|character| match character {
            '[' | '(' | '{' => '‹',
            ']' | ')' | '}' => '›',
            '|' | '"' => '\'',
            other => other,
        })
        .collect()
}

/// `Name(a, "b, c", $key=d)` → `("Name", ["a", "b, c"], opens a block)`; named `$` arguments
/// are left out.
fn call(line: &str) -> Option<(&str, Vec<String>, bool)> {
    let (name, rest) = line.split_once('(')?;
    let close = rest.rfind(')')?;
    let opens = rest[close + 1..].trim_start().starts_with('{');
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in rest[..close].chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => arguments.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    arguments.push(current);
    let arguments = arguments
        .into_iter()
        .map(|argument| argument.trim().to_owned())
        .filter(|argument| !argument.starts_with('$'))
        .collect();
    Some((name.trim(), arguments, opens))
}

/// `Class : member` lines join the block of their class, which `mermaid-text` draws.
fn class_members(source: &str) -> String {
    let mut members: Vec<(String, Vec<String>)> = Vec::new();
    let mut kept = Vec::new();
    let mut depth = 0usize;
    for line in source.lines() {
        let trimmed = line.trim();
        if depth == 0
            && let Some((class, member)) = trimmed.split_once(':')
            && is_identifier(class.trim())
            && !member.trim().is_empty()
        {
            let class = class.trim().to_owned();
            match members.iter_mut().find(|(name, _)| *name == class) {
                Some((_, list)) => list.push(member.trim().to_owned()),
                None => members.push((class, vec![member.trim().to_owned()])),
            }
            continue;
        }
        depth = (depth + trimmed.matches('{').count()).saturating_sub(trimmed.matches('}').count());
        kept.push(line.replace('~', "_"));
    }
    for (class, list) in members {
        let body = list.iter().map(|member| format!("    {member}"));
        let opening = kept.iter().position(|line| {
            let line = line.trim();
            line.strip_prefix("class ")
                .is_some_and(|rest| rest.trim_end_matches('{').trim() == class)
                && line.ends_with('{')
        });
        match opening {
            Some(index) => {
                kept.splice(index + 1..index + 1, body);
            }
            None => {
                kept.push(format!("class {class} {{"));
                kept.extend(body);
                kept.push("}".to_owned());
            }
        }
    }
    kept.join("\n")
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '~'))
}

/// C4 as a top-down flowchart: people, systems, containers and components are nodes with
/// their kind under the name, boundaries are subgraphs and relationships are labelled edges.
fn c4(source: &str) -> (Option<String>, String) {
    let mut title = None;
    let mut out = vec!["flowchart TB".to_owned()];
    for line in statements(source) {
        if line == "}" {
            out.push("end".to_owned());
            continue;
        }
        if let Some(text) = line.strip_prefix("title ") {
            title = Some(text.trim().to_owned());
            continue;
        }
        let Some((function, arguments, opens)) = call(line) else {
            continue;
        };
        let function = function.to_ascii_lowercase();
        if function.starts_with("rel") || function == "birel" {
            let arguments = if function == "relindex" {
                &arguments[1.min(arguments.len())..]
            } else {
                &arguments[..]
            };
            let [from, to, rest @ ..] = arguments else {
                continue;
            };
            let (from, to) = if function == "rel_back" {
                (to, from)
            } else {
                (from, to)
            };
            let arrow = if function == "birel" { "<-->" } else { "-->" };
            out.push(match rest.first() {
                Some(text) => format!("    {from} {arrow}|{}| {to}", label(text)),
                None => format!("    {from} {arrow} {to}"),
            });
            continue;
        }
        let Some(alias) = arguments.first() else {
            continue;
        };
        let name = arguments
            .get(1)
            .map_or_else(|| alias.clone(), |name| label(name));
        if opens {
            out.push(format!("subgraph {alias} [{name}]"));
            continue;
        }
        let kind = c4_kind(&function, arguments.get(2));
        let (open, close) = if function.contains("db") {
            ("[(", ")]")
        } else if function.starts_with("person") {
            ("([", "])")
        } else {
            ("[", "]")
        };
        out.push(format!("    {alias}{open}{name}<br/>«{kind}»{close}"));
    }
    (title, out.join("\n"))
}

/// What a C4 element is: `person`, `external system`, `container: Rust`…
fn c4_kind(function: &str, technology: Option<&String>) -> String {
    let base = function.trim_end_matches("_ext");
    let base = ["db", "queue"]
        .iter()
        .fold(base.to_owned(), |base, suffix| {
            base.strip_suffix(suffix).unwrap_or(&base).to_owned()
        });
    let base = match base.as_str() {
        "deployment_node" | "node" | "node_l" | "node_r" => "node".to_owned(),
        other => other.to_owned(),
    };
    let kind = if function.ends_with("_ext") {
        format!("external {base}")
    } else {
        base
    };
    let containers = ["container", "component"];
    match technology {
        Some(technology)
            if containers.iter().any(|kind| function.starts_with(kind))
                && !technology.is_empty() =>
        {
            format!("{kind}: {}", label(technology))
        }
        _ => kind,
    }
}

/// What a block in a ZenUML diagram closes with.
enum ZenBlock {
    /// A call with a body: the callee makes the calls inside it.
    Call,
    /// `if`, `while`, `par`, `try`…: an `end` in the sequence diagram.
    Fragment,
    /// A `group` of participants: nothing to close.
    Group,
}

/// ZenUML as a sequence diagram: `A->B: text`, `B.method(x) { … }` calls whose body is made
/// by the callee, `return`, `if`/`else`, loops, `par`, `opt` and `try`/`catch`/`finally`.
fn zenuml(source: &str) -> (Option<String>, String) {
    let mut title = None;
    let mut out = vec!["sequenceDiagram".to_owned()];
    let mut callers = vec!["Client".to_owned()];
    let mut blocks = Vec::new();
    for line in statements(source) {
        let line = line.split("//").next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(text) = line.strip_prefix("title ") {
            title = Some(text.trim().to_owned());
            continue;
        }
        if let Some(rest) = line.strip_prefix('}') {
            let rest = rest.trim().trim_end_matches('{').trim();
            if let Some(condition) = rest.strip_prefix("else") {
                let condition = condition.trim().trim_start_matches("if").trim();
                out.push(
                    format!("    else {}", condition_text(condition))
                        .trim_end()
                        .to_owned(),
                );
            } else if let Some(caught) = rest.strip_prefix("catch") {
                out.push(format!("    option catch {}", condition_text(caught)));
            } else if rest.starts_with("finally") {
                out.push("    option finally".to_owned());
            } else {
                match blocks.pop() {
                    Some(ZenBlock::Call) => {
                        callers.pop();
                    }
                    Some(ZenBlock::Fragment) => out.push("    end".to_owned()),
                    Some(ZenBlock::Group) | None => {}
                }
            }
            continue;
        }
        let opens = line.ends_with('{');
        let statement = line.trim_end_matches('{').trim();
        let caller = callers.last().cloned().unwrap_or_default();
        if let Some(participant) = statement.strip_prefix('@') {
            if let Some(starter) = participant
                .strip_prefix("Starter(")
                .and_then(|rest| rest.strip_suffix(')'))
            {
                callers = vec![starter.trim().to_owned()];
                continue;
            }
            if let Some(value) = participant.strip_prefix("return") {
                zen_return(&mut out, &callers, value);
                continue;
            }
            // `@Actor Client #FFEBE6`, `@EC2 <<BFF>> OrderService`.
            let mut words = participant
                .split_whitespace()
                .filter(|word| !word.starts_with('#') && !word.starts_with("<<"));
            let kind = words.next().unwrap_or_default();
            if let Some(name) = words.next() {
                let keyword = if kind == "Actor" {
                    "actor"
                } else {
                    "participant"
                };
                out.push(format!("    {keyword} {name}"));
            }
            continue;
        }
        if let Some(value) = statement.strip_prefix("return") {
            zen_return(&mut out, &callers, value);
            continue;
        }
        let word = statement
            .split(|character: char| !character.is_alphanumeric())
            .next()
            .unwrap_or_default();
        let fragment = match word {
            "if" => Some(format!("alt {}", condition_text(&statement[2..]))),
            "while" | "for" | "forEach" | "foreach" | "loop" => {
                Some(format!("loop {}", condition_text(&statement[word.len()..])))
            }
            "opt" => Some("opt".to_owned()),
            "par" => Some("par".to_owned()),
            "try" => Some("critical try".to_owned()),
            "group" => {
                blocks.push(ZenBlock::Group);
                continue;
            }
            _ => None,
        };
        if let Some(fragment) = fragment {
            out.push(format!("    {}", fragment.trim_end()));
            if opens {
                blocks.push(ZenBlock::Fragment);
            }
            continue;
        }
        if let Some((from, rest)) = statement.split_once("->") {
            let (to, text) = rest.split_once(':').unwrap_or((rest, ""));
            out.push(format!(
                "    {}->>{}: {}",
                from.trim(),
                to.trim(),
                text.trim()
            ));
            continue;
        }
        // `[result =] [Type ]Target.method(args)` or `[result =] new Target(args)`.
        let (result, call) = match statement.split_once('=') {
            Some((result, call)) if !call.starts_with('=') && !result.contains('(') => {
                (Some(result.trim()), call.trim())
            }
            _ => (None, statement),
        };
        let (target, message) = if let Some(created) = call.strip_prefix("new ") {
            let target = created.split('(').next().unwrap_or(created).trim();
            let arguments = created.strip_prefix(target).unwrap_or_default();
            (target.to_owned(), format!("new{arguments}"))
        } else if let Some((target, method)) = call.split_once('.') {
            let target = target.split_whitespace().last().unwrap_or(target);
            (target.to_owned(), method.to_owned())
        } else {
            // A bare name declares a participant.
            if is_identifier(call) {
                out.push(format!("    participant {call}"));
            }
            continue;
        };
        out.push(format!("    {caller}->>{target}: {message}"));
        if opens {
            blocks.push(ZenBlock::Call);
            callers.push(target);
        } else if let Some(result) = result {
            let result = result.split_whitespace().last().unwrap_or(result);
            out.push(format!("    {target}-->>{caller}: {result}"));
        }
    }
    (title, out.join("\n"))
}

fn zen_return(out: &mut Vec<String>, callers: &[String], value: &str) {
    if let [.., to, from] = callers {
        out.push(format!("    {from}-->>{to}: {}", value.trim()));
    }
}

/// `(order != null)` → `order != null`.
fn condition_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .to_owned()
}

/// A Kanban board: one column per first-level line, its cards below it, side by side.
fn kanban(source: &str) -> Vec<String> {
    const WIDTH: usize = 26;
    let mut columns: Vec<(String, Vec<String>)> = Vec::new();
    let mut column_indent = None;
    for line in source.lines().skip_while(|line| {
        line.trim().is_empty() || line.trim().starts_with("%%") || !line.trim().eq("kanban")
    }) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed == "kanban" {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let (text, metadata) = card_text(trimmed);
        match column_indent {
            Some(column) if indent > column => {
                if let Some((_, cards)) = columns.last_mut() {
                    cards.push(text);
                    if !metadata.is_empty() {
                        cards.push(format!("  {metadata}"));
                    }
                }
            }
            _ => {
                column_indent = Some(indent);
                columns.push((text, Vec::new()));
            }
        }
    }
    let boxes = columns
        .iter()
        .map(|(title, cards)| {
            let mut lines = vec![format!(
                "╭─ {} {}╮",
                fit(title, WIDTH - 3),
                "─".repeat((WIDTH - 3).saturating_sub(width(&fit(title, WIDTH - 3))),)
            )];
            for card in cards {
                let (bullet, text) = match card.strip_prefix("  ") {
                    Some(metadata) => ("  ", metadata.to_owned()),
                    None => ("• ", card.clone()),
                };
                for (index, part) in wrap(&text, WIDTH - 4).into_iter().enumerate() {
                    let lead = if index == 0 { bullet } else { "  " };
                    let cell = format!(" {lead}{part}");
                    lines.push(format!(
                        "│{cell}{}│",
                        " ".repeat(WIDTH.saturating_sub(width(&cell)))
                    ));
                }
            }
            if cards.is_empty() {
                lines.push(format!("│{}│", " ".repeat(WIDTH)));
            }
            lines.push(format!("╰{}╯", "─".repeat(WIDTH)));
            lines
        })
        .collect::<Vec<_>>();
    let height = boxes.iter().map(Vec::len).max().unwrap_or(0);
    (0..height)
        .map(|row| {
            boxes
                .iter()
                .map(|lines| {
                    lines
                        .get(row)
                        .cloned()
                        .unwrap_or_else(|| " ".repeat(WIDTH + 2))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// A card: `id[Text]`, `[Text]` or `Text`, and its `@{ assigned: …, ticket: …, priority: … }`.
fn card_text(line: &str) -> (String, String) {
    let (text, metadata) = line.split_once("@{").unwrap_or((line, ""));
    let text = text.trim();
    let text = match (text.find('['), text.rfind(']')) {
        (Some(open), Some(close)) if open < close => &text[open + 1..close],
        _ => text,
    };
    let details = metadata
        .trim_end_matches('}')
        .split(',')
        .filter_map(|pair| pair.split_once(':'))
        .map(|(key, value)| (key.trim(), unquote(value.trim()).to_owned()))
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| match key {
            "assigned" => format!("@{value}"),
            _ => value,
        })
        .collect::<Vec<_>>();
    (unquote(text).to_owned(), details.join(" · "))
}

/// A value of a radar curve, with the axis it names (`{ m: 80 }`) or in axis order.
type Value = (Option<String>, f64);

/// A radar chart as bars: one row per axis and curve, scaled between `min` and `max`.
fn radar(source: &str) -> (Option<String>, Vec<String>) {
    let mut title = None;
    let mut axes: Vec<(String, String)> = Vec::new();
    let mut curves: Vec<(String, Vec<Value>)> = Vec::new();
    let (mut minimum, mut maximum) = (None, None);
    for line in statements(source) {
        let (keyword, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match keyword {
            "title" => title = Some(unquote(rest).to_owned()),
            "axis" => axes.extend(rest.split(',').map(|axis| named(axis.trim()))),
            "curve" => {
                let (name, values) = rest.split_once('{').unwrap_or((rest, ""));
                let values = values
                    .trim_end_matches('}')
                    .split(',')
                    .filter_map(|value| {
                        let (key, value) = match value.split_once(':') {
                            Some((key, value)) => (Some(key.trim().to_owned()), value),
                            None => (None, value),
                        };
                        value.trim().parse::<f64>().ok().map(|value| (key, value))
                    })
                    .collect();
                curves.push((named(name.trim()).1, values));
            }
            "max" => maximum = rest.trim().parse::<f64>().ok(),
            "min" => minimum = rest.trim().parse::<f64>().ok(),
            _ => {}
        }
    }
    let values = curves
        .iter()
        .flat_map(|(_, values)| values.iter().map(|(_, value)| *value));
    let maximum = maximum.unwrap_or_else(|| values.clone().fold(0.0, f64::max));
    let minimum = minimum.unwrap_or(0.0);
    let axis_width = axes.iter().map(|(_, name)| width(name)).max().unwrap_or(0);
    let curve_width = curves
        .iter()
        .map(|(name, _)| width(name))
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for (index, (id, axis)) in axes.iter().enumerate() {
        for (row, (curve, values)) in curves.iter().enumerate() {
            let value = values
                .iter()
                .find(|(key, _)| key.as_deref() == Some(id.as_str()))
                .or_else(|| values.get(index).filter(|(key, _)| key.is_none()))
                .map_or(0.0, |(_, value)| *value);
            let share = ((value - minimum) / (maximum - minimum).max(f64::EPSILON)).clamp(0.0, 1.0);
            let axis = if row == 0 { axis.as_str() } else { "" };
            let bar = bar(share, 30);
            lines.push(format!(
                "{axis}{}  {curve}{}  {bar}{}  {}",
                " ".repeat(axis_width - width(axis)),
                " ".repeat(curve_width - width(curve)),
                " ".repeat(30 - width(&bar)),
                number(value)
            ));
        }
    }
    let heading = title.map(|title| format!("{title} ({} – {})", number(minimum), number(maximum)));
    (heading, lines)
}

/// `id["Label"]` → `("id", "Label")`; a bare `id` is its own label.
fn named(text: &str) -> (String, String) {
    match (text.find('['), text.rfind(']')) {
        (Some(open), Some(close)) if open < close => (
            text[..open].trim().to_owned(),
            unquote(&text[open + 1..close]).to_owned(),
        ),
        _ => (text.to_owned(), unquote(text).to_owned()),
    }
}

/// A treemap as a tree: sections add up their leaves; bars compare with the largest top total.
fn treemap(source: &str) -> Vec<String> {
    struct Node {
        depth: usize,
        name: String,
        value: f64,
    }
    let mut nodes = Vec::new();
    let mut indents = Vec::new();
    for line in source
        .lines()
        .skip_while(|line| !line.trim().starts_with("treemap"))
        .skip(1)
    {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("classDef") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        while indents.last().is_some_and(|last| *last >= indent) {
            indents.pop();
        }
        let depth = indents.len();
        indents.push(indent);
        let text = trimmed.split(":::").next().unwrap_or(trimmed).trim();
        let (name, value) = match text.rsplit_once(':') {
            Some((name, value)) if value.trim().parse::<f64>().is_ok() => {
                (name, value.trim().parse().unwrap_or(0.0))
            }
            _ => (text, 0.0),
        };
        nodes.push(Node {
            depth,
            name: unquote(name).to_owned(),
            value,
        });
    }
    // Sections add up what is below them, from the bottom up.
    for index in (0..nodes.len()).rev() {
        let depth = nodes[index].depth;
        let total = nodes[index + 1..]
            .iter()
            .take_while(|node| node.depth > depth)
            .filter(|node| node.depth == depth + 1)
            .map(|node| node.value)
            .sum::<f64>();
        if total > 0.0 {
            nodes[index].value = total;
        }
    }
    let largest = nodes
        .iter()
        .filter(|node| node.depth == 0)
        .map(|node| node.value)
        .fold(0.0, f64::max);
    let labels = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut prefix = String::new();
            for depth in 1..=node.depth {
                let siblings_follow = |depth: usize| {
                    nodes[index + 1..]
                        .iter()
                        .take_while(|next| next.depth >= depth)
                        .any(|next| next.depth == depth)
                };
                prefix.push_str(if depth == node.depth {
                    if siblings_follow(depth) {
                        "├─ "
                    } else {
                        "└─ "
                    }
                } else if siblings_follow(depth) {
                    "│  "
                } else {
                    "   "
                });
            }
            format!("{prefix}{}", node.name)
        })
        .collect::<Vec<_>>();
    let column = labels.iter().map(|label| width(label)).max().unwrap_or(0);
    nodes
        .iter()
        .zip(labels)
        .map(|(node, label)| {
            format!(
                "{label}{}  {:>8}  {}",
                " ".repeat(column - width(&label)),
                number(node.value),
                bar(node.value / largest.max(f64::EPSILON), 24)
            )
            .trim_end()
            .to_owned()
        })
        .collect()
}

/// A bar of `share` (0 to 1) of `cells`, in eighths of a cell.
fn bar(share: f64, cells: usize) -> String {
    let eighths = (share.clamp(0.0, 1.0) * (cells * 8) as f64).round() as usize;
    let mut bar = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        bar.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][eighths % 8 - 1]);
    }
    bar
}

fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn width(text: &str) -> usize {
    ratatui::text::Line::from(text).width()
}

/// `text` cut to `cells` with `…`.
fn fit(text: &str, cells: usize) -> String {
    if width(text) <= cells {
        return text.to_owned();
    }
    let mut cut = String::new();
    for character in text.chars() {
        if width(&cut) + 2 > cells {
            break;
        }
        cut.push(character);
    }
    cut.push('…');
    cut
}

/// Words of `text` in lines of at most `cells`.
fn wrap(text: &str, cells: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && width(line) + 1 + width(word) > cells {
            lines.push(fit(word, cells));
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&fit(word, cells));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(source: &str) -> String {
        render(source)
            .unwrap_or_else(|| panic!("not drawn:\n{source}"))
            .join("\n")
    }

    #[test]
    fn kinds_mermaid_text_draws_still_go_to_it() {
        assert!(text("flowchart LR\n    A[Build] --> B[Deploy]").contains("Build"));
        assert!(text("pie title Pets\n    \"Dogs\" : 3\n    \"Cats\" : 1").contains("Dogs"));
        assert!(render("not a diagram at all").is_none());
    }

    #[test]
    fn front_matter_titles_the_diagram() {
        let drawn = render("---\ntitle: Release flow\n---\nflowchart LR\n    A --> B").unwrap();
        assert_eq!(drawn[0], "Release flow");
        assert!(drawn.iter().any(|line| line.contains('A')));
    }

    #[test]
    fn class_members_after_a_colon_join_their_class() {
        let drawn = text(
            "classDiagram\n    Animal <|-- Duck\n    Animal : +int age\n    Animal: +isMammal()\n    \
             class Duck{\n      +String beakColor\n    }\n    Duck : +swim()",
        );
        for member in ["Animal", "age", "isMammal()", "Duck", "beakColor", "swim()"] {
            assert!(drawn.contains(member), "{member} in\n{drawn}");
        }
    }

    #[test]
    fn c4_becomes_a_flowchart_with_boundaries() {
        let source = "C4Context\n  title System Context\n  Enterprise_Boundary(b0, \"Bank\") {\n    \
                      Person(customer, \"Customer\", \"A customer of the bank\")\n    \
                      SystemDb_Ext(mainframe, \"Mainframe\", \"Stores, everything\")\n  }\n  \
                      Rel(customer, mainframe, \"Uses\", \"HTTPS\")\n  \
                      UpdateRelStyle(customer, mainframe, $textColor=\"blue\")";
        let (title, flowchart) = c4(source);
        assert_eq!(title.as_deref(), Some("System Context"));
        assert!(flowchart.contains("subgraph b0 [Bank]"), "{flowchart}");
        assert!(
            flowchart.contains("customer([Customer<br/>«person»])"),
            "{flowchart}"
        );
        assert!(flowchart.contains("mainframe[(Mainframe<br/>«external system»)]"));
        assert!(flowchart.contains("customer -->|Uses| mainframe"));
        let drawn = text(source);
        for word in ["System Context", "Bank", "Customer", "Mainframe", "Uses"] {
            assert!(drawn.contains(word), "{word} in\n{drawn}");
        }
    }

    #[test]
    fn zenuml_becomes_a_sequence_diagram() {
        let source = "zenuml\n    title Order Service\n    @Actor Client #FFEBE6\n    \
                      @Boundary OrderController\n    // a comment\n    \
                      OrderController.post(payload) {\n      OrderService.create(payload) {\n        \
                      order = new Order(payload)\n        if(order != null) {\n          \
                      par {\n            PurchaseService.createPO(order)\n          }\n        \
                      } else {\n          @return error\n        }\n      }\n    }\n    \
                      Client->OrderController: done";
        let (title, sequence) = zenuml(source);
        assert_eq!(title.as_deref(), Some("Order Service"));
        for expected in [
            "actor Client",
            "Client->>OrderController: post(payload)",
            "OrderController->>OrderService: create(payload)",
            "OrderService->>Order: new(payload)",
            "alt order != null",
            "par",
            "OrderService->>PurchaseService: createPO(order)",
            "else",
            "OrderService-->>OrderController: error",
            "Client->>OrderController: done",
        ] {
            assert!(sequence.contains(expected), "{expected} in\n{sequence}");
        }
        let drawn = text(source);
        assert!(drawn.starts_with("Order Service"), "{drawn}");
        assert!(drawn.contains("createPO(order)"));
    }

    #[test]
    fn kanban_draws_a_column_per_list() {
        let drawn = render(
            "kanban\n  Todo\n    [Create Documentation]\n    docs[Write the blog post]\n  \
             [In progress]\n    id6[Create renderer]\n  id9[Ready for deploy]\n    \
             id8[Design grammar]@{ assigned: 'knsv', ticket: MC-2037 }",
        )
        .unwrap();
        assert!(drawn[0].contains("Todo") && drawn[0].contains("In progress"));
        assert!(drawn[0].contains("Ready for deploy"));
        let all = drawn.join("\n");
        for card in [
            "• Create Documentation",
            "• Write the blog post",
            "@knsv · MC-2037",
        ] {
            assert!(all.contains(card), "{card} in\n{all}");
        }
        let widths = drawn.iter().map(|line| width(line)).collect::<Vec<_>>();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "{all}");
    }

    #[test]
    fn radar_draws_a_bar_for_each_axis_and_curve() {
        let drawn = render(
            "radar-beta\n  title Grades\n  axis m[\"Math\"], s[\"Science\"]\n  \
             curve a[\"Alice\"]{80, 40}\n  curve b[\"Bob\"]{ s: 100, m: 20 }\n  max 100\n  min 0",
        )
        .unwrap();
        assert_eq!(drawn[0], "Grades (0 – 100)");
        let alice_math = drawn.iter().find(|line| line.starts_with("Math")).unwrap();
        assert!(alice_math.contains("Alice") && alice_math.ends_with("  80"));
        assert_eq!(alice_math.matches('█').count(), 24);
        assert!(
            drawn
                .iter()
                .any(|line| line.contains("Bob") && line.ends_with(" 100"))
        );
    }

    #[test]
    fn treemap_adds_up_its_sections() {
        let drawn = render(
            "treemap-beta\n\"Section 1\"\n    \"Leaf 1.1\": 12\n    \"Section 1.2\":::warm\n      \
             \"Leaf 1.2.1\": 12\n\"Section 2\"\n    \"Leaf 2.1\": 24",
        )
        .unwrap();
        assert!(
            drawn[0].starts_with("Section 1") && drawn[0].contains(" 24 "),
            "{drawn:?}"
        );
        assert!(drawn[1].starts_with("├─ Leaf 1.1"));
        assert!(drawn[2].starts_with("└─ Section 1.2"));
        assert!(drawn[3].starts_with("   └─ Leaf 1.2.1"));
        assert!(drawn[4].starts_with("Section 2"));
    }
}
