import { readFileSync, writeFileSync } from 'node:fs';
import { expect, test } from 'vitest';
import { z } from 'zod';
import { services, servicesSchema } from './services';

/** services.schema.json is the registry's schema as services.ts reads it, for editors: regenerate it after changing that. */
const RERUN =
	'SCHEMA_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/server/token-exchange/services.test.ts';

const path = new URL('./services.schema.json', import.meta.url);

test('services.schema.json matches the schema services.ts reads services.json with', () => {
	const expected = { $id: 'services.schema.json', ...z.toJSONSchema(servicesSchema) };

	if (process.env.SCHEMA_BLESS) writeFileSync(path, `${JSON.stringify(expected, null, '\t')}\n`);
	expect(
		JSON.parse(readFileSync(path, 'utf8')),
		`services.schema.json is out of date. Regenerate it with \`${RERUN}\``
	).toEqual(expected);
});

test('services.json has the services plugins sign in to, each at its https token endpoint', () => {
	expect(Object.keys(services).toSorted()).toEqual(['google', 'mixcloud', 'soundcloud']);
	expect(Object.values(services).map((service) => new URL(service.tokenUrl).protocol)).toEqual([
		'https:',
		'https:',
		'https:'
	]);
	expect(() =>
		servicesSchema.parse({ services: { x: { ...services.google, tokenUrl: 'http://a.example/t' } } })
	).toThrow('Invalid URL');
});
