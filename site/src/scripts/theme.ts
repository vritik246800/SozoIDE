// Theme picker: the new theme grows in a circle from the click (View Transitions) and the front of
// that wave is made of characters in its accent (canvas, driven by anime.js).
import { animate } from 'animejs';
import { waveFront } from './ascii';
import { motion, root } from './terminal';

const options = [...document.querySelectorAll<HTMLButtonElement>('[data-theme-option]')];
const canvas = document.querySelector<HTMLCanvasElement>('canvas.wave');
const front = canvas ? waveFront(canvas) : null;
let ring: ReturnType<typeof animate> | null = null;

const mark = () => {
  for (const option of options) {
    option.setAttribute('aria-pressed', String(option.dataset.themeOption === root.dataset.theme));
  }
};
// A wave skipped by a newer click still runs its update afterwards, so every update applies
// the latest choice, not the one of its own click (Docs/development/ERRORS.md E7).
let wanted = root.dataset.theme;
const apply = () => {
  root.dataset.theme = wanted;
  try {
    localStorage.setItem('sozo-theme', wanted ?? '');
  } catch {
    // Without storage the theme still changes for this visit.
  }
  mark();
};

const choose = (option: HTMLButtonElement, x: number, y: number) => {
  if (option.dataset.themeOption === wanted) return;
  wanted = option.dataset.themeOption;
  if (!('startViewTransition' in document) || !motion.matches) return apply();
  const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  document.startViewTransition(apply).ready.then(
    () => {
      root.animate(
        { clipPath: [`circle(0 at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
        { duration: 640, easing: 'cubic-bezier(0.33, 1, 0.68, 1)', pseudoElement: '::view-transition-new(root)' },
      );
      if (!front) return;
      ring?.cancel();
      front.start();
      const wave = { radius: 0 };
      let frame = 0;
      ring = animate(wave, {
        radius,
        duration: 640,
        ease: 'outCubic',
        onUpdate: () => front.draw(x, y, wave.radius, frame++ >> 2),
        onComplete: () => front.stop(),
      });
    },
    () => {
      // A newer click skipped this wave; that click runs its own.
    },
  );
};

for (const option of options) {
  option.addEventListener('click', (event) => {
    // A keyboard click (detail 0) has no pointer position: start from the middle of the button.
    const box = option.getBoundingClientRect();
    choose(option, event.detail ? event.clientX : box.left + box.width / 2, event.detail ? event.clientY : box.top + box.height / 2);
  });
}
// During a wave every click lands on <html> (E7), so find the theme button under the pointer.
root.addEventListener('click', (event) => {
  if (event.target !== root) return;
  const option = options.find((candidate) => {
    const box = candidate.getBoundingClientRect();
    return event.clientX >= box.left && event.clientX < box.right && event.clientY >= box.top && event.clientY < box.bottom;
  });
  if (option) choose(option, event.clientX, event.clientY);
});
mark();
