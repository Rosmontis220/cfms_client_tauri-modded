import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installAppViewport } from './app-viewport';

type ViewportStub = {
  height: number;
  scale: number;
  listeners: Map<string, Set<() => void>>;
  addEventListener: (type: string, listener: () => void) => void;
  removeEventListener: (type: string, listener: () => void) => void;
  emit: (type: string) => void;
};

function createViewport(height: number): ViewportStub {
  const listeners = new Map<string, Set<() => void>>();
  return {
    height,
    scale: 1,
    listeners,
    addEventListener(type, listener) {
      const bucket = listeners.get(type) ?? new Set();
      bucket.add(listener);
      listeners.set(type, bucket);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    emit(type) {
      for (const listener of listeners.get(type) ?? []) {
        listener();
      }
    },
  };
}

function setLayoutHeight(height: number): void {
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: height });
}

function heightVar(): string {
  return document.documentElement.style.getPropertyValue('--app-viewport-height');
}

function insetVar(): string {
  return document.documentElement.style.getPropertyValue('--keyboard-inset');
}

describe('installAppViewport', () => {
  let viewport: ViewportStub;

  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal('cancelAnimationFrame', () => {});
    viewport = createViewport(800);
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: viewport });
    setLayoutHeight(800);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute('style');
  });

  it('mirrors the visual viewport height onto the root element', () => {
    const dispose = installAppViewport();
    expect(heightVar()).toBe('800px');
    expect(insetVar()).toBe('0px');
    dispose();
  });

  it('exposes the keyboard inset when the visual viewport shrinks', () => {
    const dispose = installAppViewport();
    viewport.height = 500;
    viewport.emit('resize');
    expect(heightVar()).toBe('500px');
    expect(insetVar()).toBe('300px');
    dispose();
  });

  it('ignores pinch zoom, which shrinks the viewport for another reason', () => {
    const dispose = installAppViewport();
    viewport.scale = 2;
    viewport.height = 400;
    viewport.emit('resize');
    expect(heightVar()).toBe('');
    dispose();
  });

  it('clears the variables when disposed', () => {
    const dispose = installAppViewport();
    expect(heightVar()).toBe('800px');
    dispose();
    expect(heightVar()).toBe('');
    expect(insetVar()).toBe('');
  });

  it('does nothing without a visual viewport', () => {
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: undefined });
    const dispose = installAppViewport();
    expect(heightVar()).toBe('');
    dispose();
  });

  it('shrinks the shell when the Android host reports an IME inset', () => {
    const dispose = installAppViewport();
    window.__cfmsSetKeyboardInset?.(600);
    expect(heightVar()).toBe('200px');
    expect(insetVar()).toBe('600px');

    window.__cfmsSetKeyboardInset?.(0);
    expect(heightVar()).toBe('800px');
    expect(insetVar()).toBe('0px');
    dispose();
  });

  it('works from the host inset alone, with no visual viewport', () => {
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: undefined });
    const dispose = installAppViewport();
    window.__cfmsSetKeyboardInset?.(450);
    expect(heightVar()).toBe('350px');
    expect(insetVar()).toBe('450px');
    dispose();
  });

  it('keeps the larger of the host inset and the visual viewport shrink', () => {
    const dispose = installAppViewport();
    window.__cfmsSetKeyboardInset?.(200);
    viewport.height = 300;
    viewport.emit('resize');
    expect(heightVar()).toBe('300px');
    expect(insetVar()).toBe('500px');

    viewport.height = 100;
    viewport.emit('resize');
    expect(heightVar()).toBe('100px');
    expect(insetVar()).toBe('700px');
    dispose();
  });

  it('stops accepting host insets once disposed', () => {
    const dispose = installAppViewport();
    dispose();
    expect(window.__cfmsSetKeyboardInset).toBeUndefined();
  });
});
