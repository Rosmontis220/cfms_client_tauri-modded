// CFMS Client — phone viewport contribution.
//
// Pins the application shell to the visible area so bottom composers stay above
// the on-screen keyboard and the last row clears the gesture bar. The hooks use
// a dynamic `import()` because the registry must stay inert at module scope.

import type { CustomModule } from '../types';

let disposeViewport: (() => void) | null = null;

export const appViewport: CustomModule = {
  id: 'app-viewport',
  async startup() {
    const { installAppViewport } = await import('$lib/app-viewport');
    disposeViewport?.();
    disposeViewport = installAppViewport();
  },
  dispose() {
    disposeViewport?.();
    disposeViewport = null;
  },
};
