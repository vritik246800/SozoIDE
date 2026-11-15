// The words, themes and keys of the landing page (split from index.astro, TODO 6.4.6).

// The keys of the demo in the hero (src/scripts/demo.ts).
export const demoKeys = [
  { key: 'git', keys: ['Ctrl', 'G'], label: 'Source Control', toggle: true },
  { key: 'database', keys: ['Ctrl', 'D'], label: 'Database', toggle: true },
  { key: 'search', keys: ['Ctrl', 'P'], label: 'Search', toggle: true },
  { key: 'help', keys: ['F1'], label: 'Help', toggle: true },
  { key: 'tab', keys: ['Tab'], label: 'Next tab', toggle: false },
  { key: 'escape', keys: ['Esc'], label: 'Back', toggle: false },
];
export const panels = ['git-changes', 'git-graph', 'db-query', 'db-results', 'search'] as const;

// Real terminal themes: background, foreground and ANSI 0-15 (Docs/development/DESIGN.md §2.2).
export const themes = [
  { id: 'campbell', name: 'Campbell', from: 'Windows Terminal', bg: '#0C0C0C', fg: '#CCCCCC',
    ansi: ['#0C0C0C', '#C50F1F', '#13A10E', '#C19C00', '#0037DA', '#881798', '#3A96DD', '#CCCCCC',
      '#767676', '#E74856', '#16C60C', '#F9F1A5', '#3B78FF', '#B4009E', '#61D6D6', '#F2F2F2'] },
  { id: 'iterm2', name: 'iTerm2', from: 'macOS', bg: '#000000', fg: '#C7C7C7',
    ansi: ['#000000', '#C91B00', '#00C200', '#C7C400', '#0225C7', '#CA30C7', '#00C5C7', '#C7C7C7',
      '#686868', '#FF6E67', '#5FFA68', '#FFFC67', '#6871FF', '#FF77FF', '#60FDFF', '#FFFFFF'] },
  { id: 'ubuntu', name: 'Ubuntu', from: 'GNOME Terminal', bg: '#300A24', fg: '#FFFFFF',
    ansi: ['#2E3436', '#CC0000', '#4E9A06', '#C4A000', '#3465A4', '#75507B', '#06989A', '#D3D7CF',
      '#555753', '#EF2929', '#8AE234', '#FCE94F', '#729FCF', '#AD7FA8', '#34E2E2', '#EEEEEC'] },
  { id: 'gruvbox', name: 'Gruvbox', from: 'Dark', bg: '#282828', fg: '#EBDBB2',
    ansi: ['#282828', '#CC241D', '#98971A', '#D79921', '#458588', '#B16286', '#689D6A', '#A89984',
      '#928374', '#FB4934', '#B8BB26', '#FABD2F', '#83A598', '#D3869B', '#8EC07C', '#EBDBB2'] },
  { id: 'dracula', name: 'Dracula', from: 'Dark', bg: '#282A36', fg: '#F8F8F2',
    ansi: ['#21222C', '#FF5555', '#50FA7B', '#F1FA8C', '#BD93F9', '#FF79C6', '#8BE9FD', '#F8F8F2',
      '#6272A4', '#FF6E6E', '#69FF94', '#FFFFA5', '#D6ACFF', '#FF92DF', '#A4FFFF', '#FFFFFF'] },
  // A light one too (Q14, a): One Half Light, as Windows Terminal ships it (Solarized Light's
  // foreground does not reach the WCAG AA contrast on its background).
  { id: 'one-half-light', name: 'One Half', from: 'Light', bg: '#FAFAFA', fg: '#383A42', light: true,
    ansi: ['#383A42', '#E45649', '#50A14F', '#C18301', '#0184BC', '#A626A4', '#0997B3', '#FAFAFA',
      '#4F525D', '#DF6C75', '#98C379', '#E4C07A', '#61AFEF', '#C577DD', '#56B5C1', '#FFFFFF'] },
];

export const features = [
  { keys: [['Ctrl', 'B']], title: 'Explorer and editor',
    text: 'Browse the project tree as in a file explorer, with an icon for every file type and the keys you expect (arrows fold, F2 renames, Delete goes to the Trash and Ctrl+Z undoes); mark several, find any name with /, drag onto a folder to move; right-click to create a file or a folder, or to rename or delete one, and cut, copy and paste them with the same menu or Ctrl+X, Ctrl+C and Ctrl+V; Properties shows the size, dates and permissions, and counts a big folder in the background. Changed files carry their Git letter, as in VS Code, and changes made by other programs show up by themselves. Open files in tabs that scroll when there are many; drag a tab to an edge of the editor to split it side by side or stacked, each side with its own tabs. Long lines, and long names in the Explorer, scroll sideways. Select text with the mouse or Shift and the arrows, and copy, cut or paste it with Ctrl+C, Ctrl+X, Ctrl+V or a right-click; Ctrl+Z undoes the last edit. Edits save themselves after two idle seconds and before you switch tabs or quit. Syntax highlighting covers more than 30 languages, from Rust and Go to SQL, JSP, Astro and Mermaid, and configuration files by their name, from .env and Cargo.lock to Dockerfile, with its own colour for keywords, types, functions, strings and numbers, down to the last line of long files. Markdown is drawn as you read it: headings in their own size and colour, quotes, rules, framed code blocks, Mermaid diagrams in box-drawing characters (C4, ZenUML, Kanban, Radar and Treemap too) and aligned tables with inline formatting, with the source back under the cursor or the mouse. Pictures, animated GIFs and videos are drawn in half blocks in their own colours on any terminal, 3D models (.glb, .gltf) turn and zoom with the arrows, + and - or the mouse, and PDF files open in a tab, one page at a time. Ctrl+R saves and runs the file, or its Cargo or Go project, in the bottom terminal. Right-click a symbol to go to its declaration, type definition or implementations, and hold Ctrl over it to see it is a link. Language servers (rust-analyzer, clangd, pyright, jdtls, typescript-language-server, gopls, and those of shell, YAML, PHP, HTML, CSS, JSON and Lua) underline problems and suggest with Ctrl+Space; hold Ctrl and rest the mouse on a word for 2 seconds, or press Ctrl+K, for its type and documentation, F12 or Ctrl+click goes to a definition (in a new tab when it is in another file), Alt+F12 peeks at it without leaving the tab, Shift+F12 lists the references and Alt+Left goes back; the right-click menu shows each answer beside it, and a server that is not installed offers to install itself. F9 sets breakpoints and F5 builds and debugs Rust, C, C++, Python and Go, with the call stack and the variables in a panel.' },
  { keys: [['Ctrl', 'J'], ['Ctrl', 'Shift', 'B']], title: 'Terminals with tabs',
    text: 'A bottom and a right terminal, each with its own shell, that work as any terminal: Claude Code, vim and htop get the wheel, the mouse and every key. The right terminal opens with Ctrl+Shift+B on Windows, Ctrl+Option+B on macOS and Ctrl+Alt+B on Linux. The bottom one opens up to three shells in tabs, with + and x; the right one has two; Ctrl+PageDown and Ctrl+PageUp switch them. Drag the mouse to select text and letting go copies it; Shift+Enter writes a new line without running it. Scroll back with the mouse wheel or Page Up and Page Down, and drag the dividers to resize every panel.' },
  { keys: [['Drag']], title: 'Your own layout',
    text: 'Drag any panel by its title or a corner of its border, the editor too: a box shows exactly where it will land, on a side of another panel, its centre to swap them, or the edge of the window. Drag a border to resize it wherever it is. The rest adjusts itself, as in a tiling window manager, and each folder remembers it.' },
  { keys: [['Ctrl', 'G']], title: 'Source Control',
    text: 'Stage and commit, and scroll back through a timeline of every branch, each in its own colour: click a commit to see its files, and a file to see what was added in green, removed in red and modified in yellow. See every branch in a tree grouped by remote and folder: click one to switch to it. The Launchpad shows your branch, its upstream, how far ahead and behind it is and the files waiting to be committed. The diff numbers its lines, folds its hunks, goes side by side and stages, unstages or discards one hunk or the lines you mark. Amend, stash, tag, create and delete branches, solve a conflict block by block, and see who changed each line with Alt+G. Pull is fast-forward only, nothing is ever force-pushed, and discards, deletes, rebase and checkout ask before they run.' },
  { keys: [['Ctrl', 'D']], title: 'Database',
    text: 'Connect to SQLite, PostgreSQL and MySQL and the tables and views appear on the left. Draw them as a diagram with their keys and relations, tick columns to build a query that groups, counts, sums, sorts and filters, and get the SELECT with its joins, ready to edit, copy or run with Ctrl+Enter or F5. The rows come a thousand at a time; edit a cell, add or delete a row as a statement with parameters you see before Enter runs it, copy the results as CSV, JSON or Markdown, run EXPLAIN, and open more SQL tabs. Passwords go to the system keyring, never to a file, and a connection says whether it is encrypted.' },
  { keys: [['Ctrl', 'P'], ['Ctrl', 'Shift', 'P']], title: 'Search',
    text: 'Find as you type: in the open file every match is marked and counted, 3/12, with arrows to the next; across the whole folder the search runs in the background and groups the results by file. Files listed in .gitignore stay out of the results. Aa, whole words and regular expressions; Ctrl+H replaces, in a whole folder only after a list and Enter. Ctrl+O opens a file by a few letters, Ctrl+Shift+O lists every command.' },
  { keys: [['F1'], ['F2']], title: 'Help and About',
    text: 'Every command in boxed sections, in the colours of your terminal and with the keys your system types (fn+F1 on a Mac). A short tutorial on the first start, notes on what is new in every version since yours after an update, and in About the flower grows in the middle, a button opens the repository to contribute and another shows the notes of every version, only what is new or only what was fixed. Change any shortcut right in Help: click its key, press the new one, and Reset brings back the ones SozoIDE came with. The Dependencies tab lists every tool SozoIDE and your projects use with its version, and installs or updates one with your package manager only when you press Enter.' },
  { keys: [['F7'], ['Ctrl', 'F5']], title: 'Build and run',
    text: 'F7 builds the project, Shift+F7 cleans it, Ctrl+F7 cleans and builds it and Ctrl+F5 runs it, in the bottom terminal, and Shift+F5 stops it. SozoIDE finds the project by its manifest (Cargo, Go, npm, pnpm, yarn and bun scripts, CMake, Make, Maven, Gradle, .NET, Swift, Flutter, Django, Laravel, Rails, Docker Compose) and uses only the commands it has; your own go in .sozo.toml. The first time, it shows the command, the folder and where it comes from before anything runs, and a folder with several projects asks which one.' },
];

// The tour under the keys: real captures (site/scripts/capture.sh), each opening in a popout.
export const tour = [
  { name: 'editor', title: 'Editor and terminals', text: 'Highlighting for 30+ languages, Markdown drawn as you read it and a shell under the code.' },
  { name: 'intellisense', title: 'IntelliSense', text: 'The language server suggests as you type and underlines what is wrong; hold Ctrl over a word for its documentation, F12 jumps to the definition, Alt+F12 peeks at it and Shift+F12 lists the references; the right-click menu shows the answers beside it, and a missing server offers to install itself.' },
  { name: 'menu', title: 'Answers beside the menu', text: 'Right-click a word and move over Go to Definition or Find References: the answers show beside the menu, and a click opens one in its tab.' },
  { name: 'run', title: 'Run with Ctrl+R', text: 'Save and run the file, or its Cargo or Go project, in the bottom terminal.' },
  { name: 'git-changes', title: 'Source Control', text: 'Stage and commit, with the Launchpad of your branch and its upstream.' },
  { name: 'git-graph', title: 'Commit timeline', text: 'Every branch in its own colour; a commit slides its files in from the right.' },
  { name: 'db-query', title: 'Database diagram and builder', text: 'Tables with their keys and relations; tick columns and the SQL writes itself.' },
  { name: 'db-results', title: 'Query results', text: 'Run with Ctrl+Enter or F5; the rows scroll down and sideways.' },
  { name: 'search', title: 'Find as you type', text: 'Every match marked and counted, 3/12, in the file or the whole folder.' },
] as const;

// The colours of every theme as CSS variables, for every page.
export const themeCss = [
  ...themes.map((theme, index) => {
    const { id, bg, fg, ansi } = theme;
    const selector = index === 0 ? `:root, :root[data-theme="${id}"]` : `:root[data-theme="${id}"]`;
    // A light theme is light for the browser too, and takes the blue as its accent.
    // Its white (c7) is its background: the muted text takes its dark grey (c8) instead.
    const light = 'light' in theme ? ' color-scheme: light; --accent: var(--c4); --muted: var(--c8);' : '';
    return `${selector} { --bg: ${bg}; --fg: ${fg}; ${ansi.map((color, slot) => `--c${slot}: ${color};`).join(' ')}${light} }`;
  }),
  ...themes[0].ansi.map((_, slot) => `.f${slot} { --cell: var(--c${slot}); color: var(--cell); }`),
].join('\n');

// Only the site's own scripts, styles, pictures and fonts (TODO 6.4.1); `vercel.json` sends the
// same policy as a header, with `frame-ancestors`, which a <meta> cannot carry.
export const csp = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'none'; upgrade-insecure-requests";
