import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '../../../../lexicons');

/**
 * Every JSON document in the repo's lexicons/, read from disk. This is what `#lexicon-files` resolves to outside Vite
 * (the dev network and the other tsx scripts); under Vite it's src/lib/server/atproto/lexicon-files.ts.
 */
export const LEXICON_FILES: unknown[] = readdirSync(root, { recursive: true, encoding: 'utf8' })
	.filter((file) => file.endsWith('.json'))
	.map((file) => JSON.parse(readFileSync(join(root, file), 'utf8')));
