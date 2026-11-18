import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const lexicons = join(import.meta.dirname, '../../../../lexicons');

/**
 * Where the app's lexicons are: ours, and the other authorities' as `pnpm lexicons:install` installs them from the
 * network, pinned by lexicons/network.json. The few files left beside ours that locallibrary still compiles in aren't
 * read, so the app always has the installed versions.
 */
export const LEXICON_ROOTS = [join(lexicons, 'computer'), join(lexicons, '.network')];

/**
 * Every JSON document in LEXICON_ROOTS, read from disk. This is what `#lexicon-files` resolves to outside Vite (the
 * dev network and the other tsx scripts); under Vite it's src/lib/server/atproto/lexicon-files.ts.
 */
export const LEXICON_FILES: unknown[] = LEXICON_ROOTS.flatMap((root) =>
	readdirSync(root, { recursive: true, encoding: 'utf8' })
		.filter((file) => file.endsWith('.json'))
		.map((file) => JSON.parse(readFileSync(join(root, file), 'utf8')))
);
