/**
 * Writes FlagKey from the repo's flags/catalog/, so a flag that's gone fails the type check wherever it's still used.
 * It's generated, not checked in, so that adding a flag only adds a file. `pnpm install` and `pnpm check` run this.
 */
import { readdirSync, writeFileSync } from 'node:fs';

const catalog = new URL('../../../flags/catalog/', import.meta.url);

const target = new URL('../src/lib/flags/keys.generated.ts', import.meta.url);

const keys = readdirSync(catalog)
	.filter((file) => file.endsWith('.json'))
	.map((file) => `'${file.slice(0, -'.json'.length)}'`)
	.toSorted();

writeFileSync(target, `export type FlagKey = ${keys.join(' | ') || 'never'};\n`);
