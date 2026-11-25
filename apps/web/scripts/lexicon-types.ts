/**
 * Generates src/lib/lexicons from ours (lexicons/computer/) and the other authorities' that `pnpm lexicons:install`
 * put in lexicons/network/. `lex build` reads one folder, and the three files locallibrary still compiles in from
 * lexicons/ would duplicate installed ones there, so both trees are copied into a temporary folder for it.
 * `pnpm install` and `pnpm check` run this.
 */
import { execFileSync } from 'node:child_process';
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { LEXICON_ROOTS } from './lib/lexicon-files.ts';

// `l.space` types one collection per space under TypeScript 6, so a space of two kinds of records doesn't
// type-check. Nothing here reads these bindings.
const SPACE_TYPES = 'computer.localnetwork.space.*';

const folder = mkdtempSync(join(tmpdir(), 'lexicons-'));

try {
	for (const root of LEXICON_ROOTS) cpSync(root, folder, { recursive: true });
	execFileSync(
		'lex',
		['build', '--lexicons', folder, '--out', 'src/lib/lexicons', '--clear', '--no-pretty', '--exclude', SPACE_TYPES],
		{
			stdio: 'inherit'
		}
	);
} finally {
	rmSync(folder, { recursive: true, force: true });
}
