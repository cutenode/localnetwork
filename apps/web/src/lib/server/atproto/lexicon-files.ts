/**
 * Every JSON document of ours (lexicons/computer/) and of the other authorities' that `pnpm lexicons:install` puts in
 * lexicons/network/, bundled in when Vite builds or serves the app (the production image doesn't ship either folder).
 * The few files left beside ours that locallibrary still compiles in aren't read. Vite resolves `#lexicon-files` here
 * (vite.config.ts). Scripts run with tsx, which has no `import.meta.glob`, so Node resolves it to
 * scripts/lib/lexicon-files.ts, which reads the same files from disk.
 */
export const LEXICON_FILES: unknown[] = Object.values(
	import.meta.glob(['../../../../../../lexicons/computer/**/*.json', '../../../../../../lexicons/network/**/*.json'], {
		import: 'default',
		eager: true
	})
);
