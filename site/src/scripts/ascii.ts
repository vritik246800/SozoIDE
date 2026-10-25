// Canvas effects drawn with terminal characters in the colors of the theme (DESIGN.md §2.5).
import { GLYPHS, color, motion } from './terminal';

const FONT = 'ui-monospace, "SF Mono", "Cascadia Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace';

/** Sizes a canvas to its CSS box at the device pixel ratio and measures one character cell. */
function fit(canvas: HTMLCanvasElement, context: CanvasRenderingContext2D, size: number) {
  const ratio = Math.min(devicePixelRatio || 1, 2);
  const { width, height } = canvas.getBoundingClientRect();
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.font = `${size}px ${FONT}`;
  context.textBaseline = 'middle';
  context.textAlign = 'center';
  return { width, height, cell: { w: context.measureText('M').width, h: size * 1.25 } };
}

/** A stable pseudo-random number in [0, 1) for a cell, so glyphs flicker without allocating. */
const noise = (x: number, y: number, seed: number) => {
  const value = Math.sin(x * 127.1 + y * 311.7 + seed * 74.7) * 43758.5453;
  return value - Math.floor(value);
};

/**
 * The front of the theme wave: a ring of characters in the accent of the new theme, drawn just
 * inside the circle that the new theme grows in.
 */
export function waveFront(canvas: HTMLCanvasElement) {
  const context = canvas.getContext('2d')!;
  let size = { width: 0, height: 0, cell: { w: 8, h: 16 } };
  return {
    start() {
      canvas.hidden = false;
      size = fit(canvas, context, 14);
    },
    draw(x: number, y: number, radius: number, frame: number) {
      const { width, height, cell } = size;
      context.clearRect(0, 0, width, height);
      context.fillStyle = color(6);
      const band = cell.h * 3;
      for (let row = 0; row * cell.h < height; row++) {
        for (let column = 0; column * cell.w < width; column++) {
          const cx = column * cell.w + cell.w / 2;
          const cy = row * cell.h + cell.h / 2;
          const depth = radius - Math.hypot(cx - x, cy - y);
          if (depth < 0 || depth > band) continue;
          const pick = noise(column, row, frame);
          if (pick < depth / band) continue;
          context.globalAlpha = 1 - depth / band;
          context.fillText(GLYPHS[Math.floor(pick * GLYPHS.length)], cx, cy);
        }
      }
      context.globalAlpha = 1;
    },
    stop() {
      context.clearRect(0, 0, size.width, size.height);
      canvas.hidden = true;
    },
  };
}

/**
 * Stars made of characters behind the footer logo: they twinkle and drift a little toward the
 * pointer, and only move while the footer is on screen.
 */
export function starField(canvas: HTMLCanvasElement) {
  const context = canvas.getContext('2d')!;
  let size = fit(canvas, context, 12);
  const stars = Array.from({ length: 140 }, (_, index) => ({
    x: noise(index, 1, 3),
    y: noise(index, 2, 5),
    depth: 0.2 + noise(index, 3, 7) * 0.8,
    glyph: '..::+*'[Math.floor(noise(index, 4, 11) * 6)],
    bright: noise(index, 5, 13) > 0.9,
    phase: noise(index, 6, 17) * Math.PI * 2,
  }));
  const pointer = { x: 0, y: 0 };
  let frame = 0;
  let visible = false;

  const draw = (seconds: number) => {
    const { width, height } = size;
    context.clearRect(0, 0, width, height);
    const [dim, bright] = [color(8), color(6)];
    for (const star of stars) {
      const x = (star.x * width + pointer.x * star.depth * 24 + width) % width;
      const y = star.y * height + pointer.y * star.depth * 12;
      context.globalAlpha = 0.45 + 0.55 * Math.abs(Math.sin(seconds * star.depth * 1.6 + star.phase));
      context.fillStyle = star.bright ? bright : dim;
      context.fillText(star.glyph, x, y);
    }
    context.globalAlpha = 1;
  };
  const loop = (time: number) => {
    draw(time / 1000);
    frame = visible && motion.matches ? requestAnimationFrame(loop) : 0;
  };
  const wake = () => {
    if (!frame && visible && motion.matches) frame = requestAnimationFrame(loop);
    if (!motion.matches) draw(0);
  };

  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    wake();
  }).observe(canvas);
  addEventListener('resize', () => {
    size = fit(canvas, context, 12);
    draw(performance.now() / 1000);
  });
  addEventListener('pointermove', (event) => {
    pointer.x = event.clientX / innerWidth - 0.5;
    pointer.y = event.clientY / innerHeight - 0.5;
  });
  motion.addEventListener('change', wake);
  // The theme changed: repaint with its colors even when nothing moves.
  new MutationObserver(() => draw(performance.now() / 1000)).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });
  draw(0);
}
