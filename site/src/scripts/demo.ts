// The hero screen answers the keys of SozoIDE with the transitions of the TUI (DESIGN.md §1.3):
// another workspace is drawn line by line from the top, another tab comes in from its side, and
// dialogs unfold from their middle row over a dimmed screen.
import { animate } from 'animejs';
import { COLUMNS, ROWS, motion } from './terminal';

type Workspace = 'editor' | 'git' | 'database';
type Dialog = 'search' | 'help';
type Key = 'git' | 'database' | 'search' | 'help' | 'tab' | 'escape';

const ROW = 1.18; // em: the line height of a screen row

const names: Record<string, string> = {
  editor: 'the editor with src/lib.rs and cargo test in the bottom terminal',
  'git-changes': 'Source Control, Changes tab: one staged, one modified and one new file',
  'git-graph': 'Source Control, Commit Graph tab: a timeline of two branches, with the files of the merge',
  'db-query': 'Database, Diagram tab: the crews and launches tables of a SQLite file and their relation',
  'db-results': 'Database, Builder tab: launches counted per crew, and the results below',
  search: 'Find in the file: every match of “launch” marked, the first one current',
  help: 'Help with every command',
};

const demo = document.querySelector<HTMLElement>('[data-demo]');
if (demo) start(demo);

function start(demo: HTMLElement) {
  const stage = demo.querySelector<HTMLElement>('.stage')!;
  const status = demo.querySelector<HTMLElement>('[data-demo-status]')!;
  const buttons = [...demo.querySelectorAll<HTMLButtonElement>('[data-demo-key]')];
  const screen = (name: string) => stage.querySelector<HTMLElement>(`[data-screen="${name}"]`)!;
  const tabs = { git: ['git-changes', 'git-graph'], database: ['db-query', 'db-results'] };
  const state = {
    workspace: 'editor' as Workspace,
    tab: { git: 0, database: 0 },
    dialog: null as Dialog | null,
  };
  let shown = screen('editor');
  let running: ReturnType<typeof animate> | null = null;

  // A key during a transition finishes it first, so the screen always matches the state.
  const finish = () => {
    running?.complete();
    running = null;
  };
  const current = () =>
    state.workspace === 'editor' ? 'editor' : tabs[state.workspace][state.tab[state.workspace]];

  function swap(next: HTMLElement, edge: 'top' | 'left' | 'right') {
    finish();
    const previous = shown;
    shown = next;
    if (previous === next) return;
    next.hidden = false;
    next.classList.add('arriving');
    const done = () => {
      previous.hidden = true;
      next.classList.remove('arriving');
      next.style.clipPath = '';
    };
    if (!motion.matches) return done();
    const progress = { value: 0 };
    const clip = () => {
      if (edge === 'top') {
        const rows = Math.round(progress.value * ROWS);
        next.style.clipPath = `inset(0 0 ${((ROWS - rows) / ROWS) * 100}% 0)`;
        return;
      }
      const hidden = ((COLUMNS - Math.round(progress.value * COLUMNS)) / COLUMNS) * 100;
      next.style.clipPath = edge === 'right' ? `inset(0 0 0 ${hidden}%)` : `inset(0 ${hidden}% 0 0)`;
    };
    clip();
    running = animate(progress, {
      value: 1,
      duration: edge === 'top' ? 220 : 160,
      ease: edge === 'top' ? 'linear' : 'outCubic',
      onUpdate: clip,
      onComplete: done,
    });
  }

  // Only the rows that have unfolded from the middle show, with the border on their edges.
  function unfold(dialog: HTMLElement, open: boolean, done?: () => void) {
    finish();
    const height = Number(dialog.dataset.height);
    const [top, body, bottom] = [...dialog.children] as HTMLElement[];
    const progress = { value: open ? 0 : 1 };
    const place = () => {
      const visible = progress.value <= 0 ? 0 : Math.max(2, Math.round(progress.value * height));
      const offset = Math.floor((height - visible) / 2);
      dialog.style.visibility = visible ? '' : 'hidden';
      top.style.top = `${offset * ROW}em`;
      bottom.style.top = `${(offset + visible - 1) * ROW}em`;
      body.style.clipPath = `inset(${offset * ROW}em 0 ${(height - offset - visible) * ROW}em 0)`;
    };
    if (!motion.matches) {
      progress.value = open ? 1 : 0;
      place();
      done?.();
      return;
    }
    place();
    running = animate(progress, {
      value: open ? 1 : 0,
      duration: open ? 180 : 120,
      ease: 'outCubic',
      onUpdate: place,
      onComplete: () => done?.(),
    });
  }

  // Straight to `next`, as the TUI draws the search bar and its marks.
  function show(next: HTMLElement) {
    finish();
    if (shown === next) return;
    next.hidden = false;
    shown.hidden = true;
    shown = next;
  }

  function openDialog(name: Dialog) {
    state.dialog = name;
    const dialog = screen(name);
    stage.classList.add('dimmed');
    dialog.hidden = false;
    unfold(dialog, true);
  }

  function closeDialog() {
    const dialog = screen(state.dialog!);
    state.dialog = null;
    unfold(dialog, false, () => {
      dialog.hidden = true;
      stage.classList.remove('dimmed');
    });
  }

  const actions: Record<Key, (back?: boolean) => void> = {
    git: () => toggle('git'),
    database: () => toggle('database'),
    // The search bar leaves the editor in view with every match marked: a screen of its own.
    search: () => {
      if (state.dialog || state.workspace !== 'editor') return;
      state.dialog = 'search';
      show(screen('search'));
    },
    help: () => {
      if (state.dialog === 'help') closeDialog();
      else if (!state.dialog) openDialog('help');
    },
    tab: (back = false) => {
      if (state.dialog || state.workspace === 'editor') return;
      const list = tabs[state.workspace];
      const index = state.tab[state.workspace];
      const next = (index + (back ? list.length - 1 : 1)) % list.length;
      state.tab[state.workspace] = next;
      swap(screen(list[next]), next > index ? 'right' : 'left');
    },
    escape: () => {
      if (state.dialog === 'search') {
        state.dialog = null;
        show(screen(current()));
      } else if (state.dialog) closeDialog();
      else if (state.workspace !== 'editor') toggle(state.workspace);
    },
  };

  function toggle(workspace: Exclude<Workspace, 'editor'>) {
    // As in the TUI, an open dialog keeps the keys.
    if (state.dialog) return;
    state.workspace = state.workspace === workspace ? 'editor' : workspace;
    swap(screen(current()), 'top');
  }

  function sync() {
    for (const button of buttons) {
      const key = button.dataset.demoKey as Key;
      const pressed = key === state.workspace || key === state.dialog;
      if (key === 'git' || key === 'database' || key === 'search' || key === 'help') {
        button.setAttribute('aria-pressed', String(pressed));
      }
      const idle =
        (key === 'tab' && (state.workspace === 'editor' || state.dialog !== null)) ||
        (key === 'escape' && state.workspace === 'editor' && !state.dialog) ||
        ((key === 'git' || key === 'database' || key === 'search') && state.dialog !== null) ||
        (key === 'search' && state.workspace !== 'editor');
      button.setAttribute('aria-disabled', String(idle));
    }
    status.textContent = `SozoIDE showing ${names[state.dialog ?? current()]}.`;
  }

  const press = (key: Key, back = false) => {
    // The boot of the hero (motion.ts) jumps to its end on the first key.
    demo.dispatchEvent(new CustomEvent('demo:key', { bubbles: true }));
    actions[key](back);
    sync();
  };

  for (const button of buttons) {
    button.addEventListener('click', (event) => press(button.dataset.demoKey as Key, event.shiftKey));
  }
  demo.addEventListener('keydown', (event) => {
    const letter = event.key.toLowerCase();
    const key: Key | null =
      event.ctrlKey && !event.altKey && !event.metaKey && letter === 'g' ? 'git'
      : event.ctrlKey && !event.altKey && !event.metaKey && letter === 'd' ? 'database'
      : event.ctrlKey && !event.altKey && !event.metaKey && letter === 'p' && !event.shiftKey ? 'search'
      : event.key === 'F1' ? 'help'
      : event.key === 'Escape' ? 'escape'
      : null;
    if (!key) return;
    event.preventDefault();
    press(key);
  });
  sync();
}
