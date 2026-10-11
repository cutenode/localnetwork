import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const root = join(import.meta.dirname, '../../..');

// The site's copy, code and the lexicons it publishes are written without long dashes.
const folders = ['apps/web/src', 'lexicons/computer'];

// Files only: some route folders are named like files (`oauth-client-metadata.json/`).
const files = folders.flatMap((folder) =>
	readdirSync(join(root, folder), { recursive: true, withFileTypes: true })
		.filter((entry) => entry.isFile() && /\.(svelte|ts|js|css|html|json)$/.test(entry.name))
		.map((entry) => join(entry.parentPath, entry.name))
);

// Built from its code point, so this file doesn't contain one (the formatter unescapes \u escapes).
const EM_DASH = String.fromCodePoint(0x2014);

describe('writing', () => {
	it('has no em dashes', () => {
		const offenders = files.filter((file) => readFileSync(file, 'utf8').includes(EM_DASH));

		expect(files.length).toBeGreaterThan(100);
		expect(offenders.map((file) => relative(root, file))).toEqual([]);
	});
});
