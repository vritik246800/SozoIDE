// Motion of the page with GSAP (DESIGN.md §2.5): the hero boots like `sozo .` in a terminal — the
// prompt, the splash printed line by line with its loading bar, then the IDE drawn over it — and
// the rest rises and decodes from ASCII characters as it scrolls into view.
import { gsap } from 'gsap';
import { ScrambleTextPlugin } from 'gsap/ScrambleTextPlugin';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { GLYPHS, ROWS, root, sweepBar } from './terminal';

gsap.registerPlugin(ScrollTrigger, ScrambleTextPlugin);

const decode = { scrambleText: { text: '{original}', chars: GLYPHS, speed: 0.5 }, duration: 0.6 };

/** `sozo .` in an empty terminal, the splash, and the IDE drawn over it, as in the TUI. */
function boot() {
  const timeline = gsap.timeline();
  const splash = document.querySelector<HTMLElement>('[data-screen="splash"]');
  const bar = splash?.querySelector<HTMLElement>('[data-bar]');
  if (!splash || !bar) return timeline;
  const lines = splash.querySelectorAll('.line');
  const width = Number(bar.dataset.bar);
  const rows = { covered: 0 };
  const sweep = () => {
    bar.innerHTML = sweepBar(width, timeline.time());
  };
  const stop = () => gsap.ticker.remove(sweep);

  timeline
    .set('[data-screen="editor"]', { autoAlpha: 0 })
    .set('.boot', { display: 'block', autoAlpha: 1 })
    .fromTo('.boot .typed', { width: 0 }, { width: '6ch', duration: 0.42, ease: 'steps(6)' }, 0.5)
    .set('.boot', { autoAlpha: 0 }, 1.1)
    .set(lines, { visibility: 'hidden' }, 0)
    .set(splash, { display: 'block' }, 1.1)
    .set('[data-screen="editor"]', { autoAlpha: 1 }, 1.1)
    .add(() => gsap.ticker.add(sweep), 1.1)
    // The splash clears the screen, then prints its lines one by one in 420 ms (SPLASH_DRAW).
    .to(lines, { visibility: 'visible', duration: 0, stagger: 0.42 / lines.length }, 1.1)
    .to(rows, {
      covered: ROWS,
      duration: 0.26,
      ease: `steps(${ROWS})`,
      onUpdate: () => {
        splash.style.clipPath = `inset(${(rows.covered / ROWS) * 100}% 0 0 0)`;
      },
    }, 2.3)
    .set(splash, { display: 'none' })
    .add(stop);
  return timeline;
}

gsap.matchMedia().add('(prefers-reduced-motion: no-preference)', () => {
  const words = gsap.utils.toArray<HTMLElement>('.hero .word');
  const hero = gsap
    .timeline({ defaults: { ease: 'expo.out' } })
    .from(words, { y: '0.4em', autoAlpha: 0, duration: 0.7, stagger: 0.05 }, 0)
    .to(words, { ...decode, stagger: 0.05 }, 0)
    .from(['.tagline', '.lede', '.actions', '.note'], { y: 16, autoAlpha: 0, duration: 0.7, stagger: 0.1 }, 0.25)
    .from('.window', { y: 16, autoAlpha: 0, duration: 0.8 }, 0.45)
    .add(boot(), 0.45)
    .from(['.try', ...gsap.utils.toArray('.theme-list button')], { y: 16, autoAlpha: 0, duration: 0.6, stagger: 0.06 }, 2.6);
  // "Press any key to continue": a click on the screen or a key of the demo ends the intro.
  const skip = () => {
    if (hero.progress() < 1) hero.progress(1, false);
  };
  document.querySelector('.window')?.addEventListener('pointerdown', skip);
  document.querySelector('[data-demo]')?.addEventListener('demo:key', skip);

  // Blocks rise once, in sequence when they enter together; titles decode from ASCII.
  gsap.set('.reveal', { autoAlpha: 0, y: 24 });
  ScrollTrigger.batch('.reveal', {
    start: 'top 90%',
    once: true,
    onEnter: (batch) => {
      gsap.to(batch, { autoAlpha: 1, y: 0, duration: 0.7, ease: 'expo.out', stagger: 0.09 });
      batch.forEach((block, index) => {
        if (block.matches('h2')) gsap.to(block, { ...decode, delay: index * 0.09 });
        // The keys of each tool are pressed one combination at a time, after the row arrives.
        block.querySelectorAll('.combo').forEach((combo, key) => {
          gsap.delayedCall(index * 0.09 + 0.25 + key * 0.4, () => combo.classList.add('down'));
          gsap.delayedCall(index * 0.09 + 0.45 + key * 0.4, () => combo.classList.remove('down'));
        });
      });
    },
  });

  // The ASCII art of the footer is drawn line by line, like the splash.
  gsap.utils.toArray<HTMLElement>('.draw').forEach((art) => {
    const lines = (art.textContent ?? '').split('\n').length;
    gsap.fromTo(
      art,
      { clipPath: 'inset(0 0 100% 0)' },
      { clipPath: 'inset(0 0 0% 0)', duration: 0.7, ease: `steps(${lines})`, scrollTrigger: { trigger: art, start: 'top 95%', once: true } },
    );
  });
});

// The page hid its moving parts until this script ran (data-motion="wait" in the head).
root.dataset.motion = 'on';
