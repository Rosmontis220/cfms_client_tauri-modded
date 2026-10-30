// Keeps the application shell the same size as the *visual* viewport.
//
// On Android the on-screen keyboard shrinks `window.visualViewport` but leaves
// the layout viewport (`window.innerHeight`, and therefore `height: 100%`) at
// the full window height. Any composer that lives at the bottom of a
// full-height flex column (the chat and IRC pages) then ends up underneath the
// keyboard. Mirroring the visual viewport through CSS variables lets the shell
// shrink with the keyboard so those composers stay reachable.
//
// `--app-viewport-height` — height the shell should occupy.
// `--keyboard-inset`      — how much of the window is hidden by the keyboard.

const HEIGHT_VAR = '--app-viewport-height';
const INSET_VAR = '--keyboard-inset';

/** Pinch-zoom also shrinks the visual viewport; never treat that as a keyboard. */
const MAX_TRUSTED_SCALE = 1.05;

function applyViewportHeight(height: number, layoutHeight: number): void {
  const root = document.documentElement;
  const rounded = Math.round(height);
  root.style.setProperty(HEIGHT_VAR, `${rounded}px`);
  const inset = Math.max(0, Math.round(layoutHeight - rounded));
  root.style.setProperty(INSET_VAR, `${inset}px`);
}

function clearViewportHeight(): void {
  const root = document.documentElement;
  root.style.removeProperty(HEIGHT_VAR);
  root.style.removeProperty(INSET_VAR);
}

/** Scrolls the focused field back into view once the keyboard finished resizing. */
function revealFocusedField(): void {
  const active = document.activeElement;
  if (!(active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement)) {
    return;
  }
  active.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

/**
 * Installs the visual-viewport sync. Returns a disposer; calling it twice is
 * safe and a second call while installed is a no-op.
 */
export function installAppViewport(): () => void {
  if (typeof window === 'undefined' || !window.visualViewport) {
    return () => {};
  }

  const viewport = window.visualViewport;
  let frame = 0;

  const sync = (): void => {
    frame = 0;
    if (viewport.scale > MAX_TRUSTED_SCALE) {
      // A zoomed viewport says nothing about the keyboard.
      clearViewportHeight();
      return;
    }
    const layoutHeight = window.innerHeight;
    const height = viewport.height;
    if (!(height > 0) || !(layoutHeight > 0)) {
      clearViewportHeight();
      return;
    }
    applyViewportHeight(height, layoutHeight);
    if (layoutHeight - height > 1) {
      revealFocusedField();
    }
  };

  const schedule = (): void => {
    if (frame) {
      return;
    }
    frame = window.requestAnimationFrame(sync);
  };

  sync();
  viewport.addEventListener('resize', schedule);
  viewport.addEventListener('scroll', schedule);
  window.addEventListener('resize', schedule);
  window.addEventListener('orientationchange', schedule);

  return () => {
    if (frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
    }
    viewport.removeEventListener('resize', schedule);
    viewport.removeEventListener('scroll', schedule);
    window.removeEventListener('resize', schedule);
    window.removeEventListener('orientationchange', schedule);
    clearViewportHeight();
  };
}
