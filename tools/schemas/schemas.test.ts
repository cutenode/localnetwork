// Every JSON file in the repository whose `$schema` is a schema of ours follows it. Editors read the
// same `$schema`; this holds CI to it, and a new file is checked once it names its schema.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { Ajv2020 } from 'ajv/dist/2020.js';
// A CommonJS module whose types declare its plugin as `default`, which Node's ESM import keeps on the module.
import ajvFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';

const repo = resolve(import.meta.dirname, '../..');

const read = (path: string) => readFileSync(join(repo, path), 'utf8');

// Tracked files and new ones not yet added, as tools/policy.sh lists them.
const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '*.json'], {
	cwd: repo,
	encoding: 'utf8'
})
	.split('\n')
	.filter((path) => path !== '');

// A relative `$schema` outside node_modules (the lint and format configs point at their tools'
// schemas, which the tools check themselves). Read as text first: those configs have comments.
const ownSchema = /"\$schema"\s*:\s*"(\.[^"]+)"/;

const pairs = files.flatMap((file) => {
	const schema = ownSchema.exec(read(file))?.[1];

	return schema === undefined || schema.includes('node_modules/') ? [] : [[file, join(dirname(file), schema)]];
});

describe('JSON files with a schema of ours', () => {
	it('finds them', () => {
		expect(pairs.length).toBeGreaterThan(0);
	});

	it.each(pairs)('%s follows %s', (file, schema) => {
		const validate = ajvFormats.default(new Ajv2020({ allErrors: true })).compile(JSON.parse(read(schema)));
		const data: unknown = JSON.parse(read(file));

		expect(validate(data), JSON.stringify(validate.errors)).toBe(true);
	});
});
