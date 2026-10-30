// Keeps the application shell the same size as the *visible* area.
//
// On Android the activity runs edge-to-edge, so the window does not resize when
// the on-screen keyboard opens: `window.innerHeight` (and therefore
// `height: 100%`) keeps the full height and any composer pinned to the bottom of
// a full-height column ends up underneath the keyboard. Two sources tell us how
// much is covered, and we take the larger of them:
//
//   1. `window.visualViewport` — accurate wherever the browser shrinks it.
//   2. `window.__cfmsSetKeyboardInset()` — pushed from `MainActivity` with the
//      IME WindowInset, which is the only signal that works under edge-to-edge.
//
// `--app-viewport-height` — height the shell should occupy.
// `--keyboard-inset`      — how much of the window is hidden by the keyboard.

const HEIGHT_VAR = '--app-viewport-height';
const INSET_VAR = '--keyboard-inset';

/** Pinch-zoom also shrinks the visual viewport; never treat that as a keyboard. */
const MAX_TRUSTED_SCALE = 1.05;

declare global {
  interface Window {
    /** Injected by the Android host; reports the IME inset in CSS pixels. */
    __cfmsSetKeyboardInset?: (inset: number) => void;
  }
}

function applyViewportHeight(height: number, inset: number): void {
  const root = document.documentElement;
  const rounded = Math.round(height);
  root.style.setProperty(HEIGHT_VAR, `${rounded}px`);
  root.style.setProperty(INSET_VAR, `${Math.max(0, Math.round(inset))}px`);
}

function clearViewportHeight(): void {
  const root = document.documentElement;
  root.style.removeProperty(HEIGHT_VAR);
  root.style.removeProperty(INSET_VAR);
}

/** Scrolls the focused field back into view once the shell finished shrinking. */
function revealFocusedField(): void {
  const active = document.activeElement;
  if (!(active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement)) {
    return;
  }
  active.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

/**
 * Installs the viewport sync. Returns a disposer; calling it twice is safe and a
 * second call while installed is a no-op.
 */
export function installAppViewport(): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }

  const viewport = window.visualViewport;
  let frame = 0;
  let scheduled = false;
  /** Keyboard height reported by the Android host, in CSS pixels. */
  let nativeInset = 0;

  const sync = (): void => {
    const layoutHeight = window.innerHeight;
    if (!(layoutHeight > 0)) {
      clearViewportHeight();
      return;
    }

    let inset = nativeInset;
    if (viewport && viewport.scale <= MAX_TRUSTED_SCALE && viewport.height > 0) {
      inset = Math.max(inset, layoutHeight - viewport.height);
    } else if (inset <= 0) {
      // Nothing knows about a keyboard here; leave the CSS fallback in charge.
      clearViewportHeight();
      return;
    }

    const height = layoutHeight - inset;
    if (!(height > 0)) {
      clearViewportHeight();
      return;
    }
    applyViewportHeight(height, inset);
    if (inset > 1) {
      revealFocusedField();
    }
  };

  const schedule = (): void => {
    if (scheduled) {
      return;
    }
    scheduled = true;
    frame = window.requestAnimationFrame(() => {
      scheduled = false;
      frame = 0;
      sync();
    });
  };

  // An unknown page reload leaves the host reporting a stale inset, so start
  // from zero and let the next inset dispatch correct it.
  window.__cfmsSetKeyboardInset = (inset: number) => {
    nativeInset =
      typeof inset === 'number' && Number.isFinite(inset) ? Math.max(0, Math.round(inset)) : 0;
    schedule();
  };

  sync();
  if (viewport) {
    viewport.addEventListener('resize', schedule);
    viewport.addEventListener('scroll', schedule);
  }
  window.addEventListener('resize', schedule);
  window.addEventListener('orientationchange', schedule);

  return () => {
    if (frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
    }
    scheduled = false;
    delete window.__cfmsSetKeyboardInset;
    if (viewport) {
      viewport.removeEventListener('resize', schedule);
      viewport.removeEventListener('scroll', schedule);
    }
    window.removeEventListener('resize', schedule);
    window.removeEventListener('orientationchange', schedule);
    clearViewportHeight();
  };
}
