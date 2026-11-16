import type { Kind } from './kind';

/**
 * Every kind of notification in kinds/, by path, bundled in when Vite builds or serves the app. Vite resolves
 * `#notification-kinds` here (vite.config.ts). Scripts run with tsx, which has no `import.meta.glob`, so Node resolves
 * it to scripts/lib/kind-files.ts, which imports the same files from disk.
 */
export const KIND_FILES = import.meta.glob<Kind>('./kinds/*.ts', { eager: true, import: 'default' });
