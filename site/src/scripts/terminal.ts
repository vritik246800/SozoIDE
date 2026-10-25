// Helpers shared by the motion of the page, all in the language of the TUI (DESIGN.md §2.5).

export const root = document.documentElement;
export const motion = matchMedia('(prefers-reduced-motion: no-preference)');

/** Characters that ASCII text decodes from. */
export const GLYPHS = '#%&*+=-:./\\|<>';

/** The rows and columns of every SozoIDE capture on the page. */
export const COLUMNS = 132;
export const ROWS = 34;

/** A color of the current theme: `fg`, `bg` or an ANSI slot. */
export const color = (slot: 'fg' | 'bg' | number) =>
  getComputedStyle(root).getPropertyValue(typeof slot === 'number' ? `--c${slot}` : `--${slot}`).trim();

/**
 * One row of `width` cells, as in the TUI (src/ui/components.rs): `━` in the accent from `start`
 * to `end` (in cells, to half a cell) over a `─` track in ANSI 8.
 */
export function bar(width: number, start: number, end: number) {
  const [from, to] = [Math.round(start * 2), Math.round(end * 2)];
  const filled = (half: number) => half >= from && half < to;
  let html = '';
  for (let cell = 0; cell < width; cell++) {
    const left = filled(cell * 2);
    const right = filled(cell * 2 + 1);
    const glyph = left && right ? '━' : left ? '╾' : right ? '╼' : '─';
    html += `<span class="${left || right ? 'f6' : 'f8'}">${glyph}</span>`;
  }
  return html;
}

/** The bar of work of unknown length: a segment crosses it every 1.2 s, eased in and out. */
export function sweepBar(width: number, seconds: number) {
  const phase = (seconds / 1.2) % 1;
  const eased = (1 - Math.cos(phase * Math.PI)) / 2;
  const segment = width * 0.3;
  const start = -segment + (width + segment) * eased;
  return bar(width, start, start + segment);
}
