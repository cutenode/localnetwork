import { readFileSync, writeFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { z } from 'zod';
import { FLAGS, flagDefinition, overdue, parseCatalog } from './catalog';

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
		['/flags/catalog/zines.json', { key: 'zines', description: 'Zines.', type: 'ops' }],
		['/flags/catalog/beta.json', { key: 'beta', description: 'Beta.', type: 'ops' }]
	]);

	expect(flags.map((flag) => flag.key)).toEqual(['beta', 'zines']);
});

it('refuses a file not named for its key', () => {
	const file = { key: 'newPlanner', description: 'The new planner.', type: 'release', removeBy: '2026-12-01' };

	expect(parseCatalog([['/flags/catalog/newPlanner.json', file]])).toEqual([file]);
	expect(() => parseCatalog([['/flags/catalog/planner.json', file]])).toThrow('holds the flag newPlanner');
});

it('refuses a key that would not work as a property, and a field it does not know', () => {
	expect(() =>
		parseCatalog([['/flags/catalog/new-planner.json', { key: 'new-planner', description: 'x', type: 'ops' }]])
	).toThrow('Invalid string');
	expect(() =>
		parseCatalog([['/flags/catalog/a.json', { key: 'a', description: 'x', type: 'ops', owner: 'me' }]])
	).toThrow('Unrecognized key');
	expect(() => parseCatalog([['/flags/catalog/a.json', { key: 'a', description: '', type: 'ops' }]])).toThrow(
		'Too small'
	);
});

/** Parses one flag named `a`, with `fields` on top of its key and description. */
const parseFlag = (fields: { type?: string; removeBy?: string }) =>
	parseCatalog([['/flags/catalog/a.json', { key: 'a', description: 'x', ...fields }]]);

it('refuses a flag without a type it knows', () => {
	expect(() => parseFlag({})).toThrow('Invalid discriminator value');
	expect(() => parseFlag({ type: 'killSwitch' })).toThrow('Invalid discriminator value');
});

it('wants a removeBy on release and experiment flags only', () => {
	expect(parseFlag({ type: 'release', removeBy: '2026-12-01' })).toEqual([
		{ key: 'a', description: 'x', type: 'release', removeBy: '2026-12-01' }
	]);
	expect(parseFlag({ type: 'permission' })).toEqual([{ key: 'a', description: 'x', type: 'permission' }]);
	expect(() => parseFlag({ type: 'experiment' })).toThrow('expected string, received undefined');
	expect(() => parseFlag({ type: 'ops', removeBy: '2026-12-01' })).toThrow(/Unrecognized key.*removeBy/);
});

it('refuses a removeBy that is not a real date as YYYY-MM-DD', () => {
	expect(() => parseFlag({ type: 'release', removeBy: '2026-02-30' })).toThrow('Invalid ISO date');
	expect(() => parseFlag({ type: 'release', removeBy: '2026-1-5' })).toThrow('Invalid ISO date');
});

it('finds the flags past their removeBy, and not one due today', () => {
	const flags = parseCatalog([
		[
			'/flags/catalog/newPlanner.json',
			{ key: 'newPlanner', description: 'x', type: 'release', removeBy: '2026-12-01' }
		],
		['/flags/catalog/zines.json', { key: 'zines', description: 'x', type: 'experiment', removeBy: '2026-12-02' }],
		['/flags/catalog/readOnly.json', { key: 'readOnly', description: 'x', type: 'ops' }]
	]);

	expect(overdue(flags, '2026-12-02')).toEqual([
		'newPlanner was due for removal on 2026-12-01: delete flags/catalog/newPlanner.json and the code it gates, or move removeBy'
	]);
	expect(overdue(flags, '2026-12-01')).toEqual([]);
});

it('has no flag past its removeBy', () => {
	expect(overdue(FLAGS, new Date().toISOString().slice(0, 10))).toEqual([]);
});
