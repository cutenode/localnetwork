import { readFileSync, writeFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { z } from 'zod';
import { FLAGS, flagDefinition, parseCatalog } from './catalog';

/** flags/flag.schema.json is the catalog's schema as catalog.ts reads it, for editors: regenerate it after changing that. */
const RERUN = 'SCHEMA_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/server/flags/catalog.test.ts';

const path = new URL('../../../../../../flags/flag.schema.json', import.meta.url);

it('flag.schema.json matches the schema catalog.ts reads the catalog with', () => {
	const expected = { $id: 'flag.schema.json', ...z.toJSONSchema(flagDefinition) };

	if (process.env.SCHEMA_BLESS) writeFileSync(path, `${JSON.stringify(expected, null, '\t')}\n`);
	expect(
		JSON.parse(readFileSync(path, 'utf8')),
		`flag.schema.json is out of date. Regenerate it with \`${RERUN}\``
	).toEqual(expected);
});

it('reads every flag in the catalog, in order', () => {
	expect(FLAGS.map((flag) => flag.key)).toContain('parityCanary');
	expect(FLAGS.map((flag) => flag.key)).toEqual(FLAGS.map((flag) => flag.key).toSorted((a, b) => a.localeCompare(b)));
});

it('puts flags in order, whatever order the files came in', () => {
	const flags = parseCatalog([
		['/flags/catalog/zines.json', { key: 'zines', description: 'Zines.' }],
		['/flags/catalog/beta.json', { key: 'beta', description: 'Beta.' }]
	]);

	expect(flags.map((flag) => flag.key)).toEqual(['beta', 'zines']);
});

it('refuses a file not named for its key', () => {
	const file = { key: 'newPlanner', description: 'The new planner.' };

	expect(parseCatalog([['/flags/catalog/newPlanner.json', file]])).toEqual([file]);
	expect(() => parseCatalog([['/flags/catalog/planner.json', file]])).toThrow('holds the flag newPlanner');
});

it('refuses a key that would not work as a property, and a field it does not know', () => {
	expect(() => parseCatalog([['/flags/catalog/new-planner.json', { key: 'new-planner', description: 'x' }]])).toThrow(
		'Invalid string'
	);
	expect(() => parseCatalog([['/flags/catalog/a.json', { key: 'a', description: 'x', owner: 'me' }]])).toThrow(
		'Unrecognized key'
	);
	expect(() => parseCatalog([['/flags/catalog/a.json', { key: 'a', description: '' }]])).toThrow('Too small');
});
