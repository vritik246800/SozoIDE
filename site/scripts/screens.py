"""Turns the tmux captures of site/scripts/capture.sh into site/src/data/screens.json.

Each screen is 34 rows of runs [text, classes]: fN is one of the 16 ANSI colors, b bold, d dim
(faint) and r reverse video; text without a class is the terminal foreground. The project path of
the container becomes ~/code/orbit, with the row padded so every border stays in place, and the
script refuses any capture that still names a home directory or a person.

    python3 site/scripts/screens.py target/site-captures
"""

import json
import re
import sys
from pathlib import Path

COLUMNS, ROWS = 132, 34
ROOT, SHOWN = "/home/dev/code/orbit", "~/code/orbit"
SCREENS = ["editor", "git-changes", "git-graph", "db-query", "db-results", "search", "run", "intellisense", "menu"]
DIALOGS = {"help": "Help - all commands"}
FORBIDDEN = re.compile(r"/home/|/Users/|(?i:vritik|valabdas)")
SGR = re.compile(r"\x1b\[([0-9;:]*)m")


def cells(line):
    """The cells of one captured row as (character, classes)."""
    style = {"fg": None, "b": False, "d": False, "r": False, "u": False}
    out = []
    position = 0
    for match in [*SGR.finditer(line), None]:
        end = match.start() if match else len(line)
        for character in line[position:end]:
            # Nerd Font icons are not in the page fonts: they become blanks, as on the hero.
            if 0xE000 <= ord(character) <= 0xF8FF or ord(character) > 0xFFFF:
                character = " "
            out.append((character, classes(style)))
        if not match:
            break
        position = match.end()
        # Underline colours come as 58:5:N (colons): the underline itself is enough.
        codes = [int(code) if code else 0 for code in match.group(1).split(";") if ":" not in code]
        while codes:
            code = codes.pop(0)
            if code == 0:
                style = {"fg": None, "b": False, "d": False, "r": False, "u": False}
            elif code == 4:
                style["u"] = True
            elif code == 24:
                style["u"] = False
            elif code == 58:
                codes = codes[2:] if codes[:1] == [5] else codes[4:]
            elif code in (38, 48):
                kind = codes.pop(0)
                if kind != 5:
                    raise SystemExit(f"RGB color in a capture (SGR {match.group(1)}): the UI only uses ANSI")
                color = codes.pop(0)
                if color > 15:
                    raise SystemExit(f"Color {color} is outside the 16 ANSI colors")
                if code == 38:
                    style["fg"] = color
            elif 30 <= code <= 37:
                style["fg"] = code - 30
            elif 90 <= code <= 97:
                style["fg"] = code - 90 + 8
            elif code == 39:
                style["fg"] = None
            elif code == 1:
                style["b"] = True
            elif code == 2:
                style["d"] = True
            elif code == 22:
                style["b"] = style["d"] = False
            elif code == 7:
                style["r"] = True
            elif code == 27:
                style["r"] = False
    return out


def classes(style):
    names = [f"f{style['fg']}"] if style["fg"] is not None else []
    names += [name for name in ("b", "d", "r", "u") if style[name]]
    return " ".join(names)


def neutral(row):
    """Replaces the container path by ~/code/orbit and pads the row so it keeps its width."""
    text = "".join(character for character, _ in row)
    while ROOT in text:
        start = text.index(ROOT)
        style = row[start][1]
        row[start : start + len(ROOT)] = [(character, style) for character in SHOWN]
        missing = len(ROOT) - len(SHOWN)
        text = "".join(character for character, _ in row)
        after = start + len(SHOWN)
        line = text.find("─", after)
        if line >= 0:
            row[line:line] = [row[line]] * missing
        else:
            row[after:after] = [(" ", row[after][1] if after < len(row) else "")] * missing
        text = "".join(character for character, _ in row)
    return row


def screen(path):
    lines = path.read_text(encoding="utf-8").split("\n")[:ROWS]
    rows = []
    for line in lines:
        row = neutral(cells(line))
        row += [(" ", "")] * (COLUMNS - len(row))
        if len(row) != COLUMNS:
            raise SystemExit(f"{path.name}: a row is {len(row)} cells wide, not {COLUMNS}")
        text = "".join(character for character, _ in row)
        if FORBIDDEN.search(text):
            raise SystemExit(f"{path.name}: personal path or name left in {text.strip()!r}")
        rows.append(row)
    if len(rows) != ROWS:
        raise SystemExit(f"{path.name}: {len(rows)} rows, not {ROWS}")
    return rows


def runs(row):
    merged = []
    for character, style in row:
        if merged and merged[-1][1] == style:
            merged[-1][0] += character
        else:
            merged.append([character, style])
    return merged


def dialog(rows, title):
    """The rounded box whose top border carries `title`, cut out of its capture."""
    for top, row in enumerate(rows):
        text = "".join(character for character, _ in row)
        if title not in text:
            continue
        left = text.rindex("╭", 0, text.index(title))
        right = text.index("╮", text.index(title))
        bottom = next(y for y in range(top + 1, ROWS) if rows[y][left][0] == "╰")
        return {
            "top": top,
            "left": left,
            "width": right - left + 1,
            "height": bottom - top + 1,
            "rows": [runs(rows[y][left : right + 1]) for y in range(top, bottom + 1)],
        }
    raise SystemExit(f"No dialog titled {title!r}")


def main():
    folder = Path(sys.argv[1])
    data = {
        "cols": COLUMNS,
        "rows": ROWS,
        "screens": {name: [runs(row) for row in screen(folder / f"{name}.ansi")] for name in SCREENS},
        "dialogs": {name: dialog(screen(folder / f"{name}.ansi"), title) for name, title in DIALOGS.items()},
    }
    target = Path(__file__).resolve().parents[1] / "src" / "data" / "screens.json"
    target.parent.mkdir(exist_ok=True)
    target.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{target}: {len(data['screens'])} screens and {len(data['dialogs'])} dialogs")


if __name__ == "__main__":
    main()
