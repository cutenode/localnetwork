import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '../..');

/**
 * The Lua scripts in lua/, read from disk and keyed by path as `import.meta.glob` keys them. This is what `#lua-files`
 * resolves to outside Vite (the tsx scripts); under Vite it's src/lib/server/happyview/lua-files.ts.
 */
export const LUA_FILES: Record<string, string> = Object.fromEntries(
	readdirSync(join(root, 'lua'), { recursive: true, encoding: 'utf8' })
		.filter((file) => file.endsWith('.lua'))
		.map((file) => [`/lua/${file.split('\\').join('/')}`, readFileSync(join(root, 'lua', file), 'utf8')])
);
