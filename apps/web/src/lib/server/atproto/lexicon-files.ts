/**
 * Every JSON document in the repo's lexicons/, bundled in when Vite builds or serves the app (the production image
 * doesn't ship that folder). Vite resolves `#lexicon-files` here (vite.config.ts). Scripts run with tsx, which has no
 * `import.meta.glob`, so Node resolves it to scripts/lib/lexicon-files.ts, which reads the same files from disk.
 */
export const LEXICON_FILES: unknown[] = Object.values(
	import.meta.glob('../../../../../../lexicons/**/*.json', { import: 'default', eager: true })
);
