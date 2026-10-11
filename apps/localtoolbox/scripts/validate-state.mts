#!/usr/bin/env node
// Validates exported state files against docs/manager-state.schema.json.
//
//   pnpm validate:state <file.json> [more.json …]
//
// The schema is plain JSON Schema 2020-12, so any conformant validator will
// do (check-jsonschema, an editor's schema support, a CI step); this script
// is only the convenient one, using Ajv from the dev dependencies.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import Ajv2020, { type ErrorObject } from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const ROOT = resolve(import.meta.dirname, '..');

export const SCHEMA_PATH = resolve(ROOT, 'docs/manager-state.schema.json');

export const readSchema = () => JSON.parse(readFileSync(SCHEMA_PATH, 'utf8'));

// An Ajv that reports every problem, not just the first, and understands the
// "date-time" format the schema uses.
export function createValidator(schema = readSchema()) {
	const ajv = new Ajv2020({ allErrors: true, strict: true });
	addFormats(ajv);

	return ajv.compile(schema);
}

// Ajv's errors as one readable line each.
export const explain = (errors: ErrorObject[] | null | undefined) =>
	(errors || []).map((e) => `  ${e.instancePath || '/'} ${e.message}`);

function main(files: string[]) {
	if (files.length === 0) {
		console.error('usage: pnpm validate:state <file.json> [more.json …]');

		return 2;
	}

	const validate = createValidator();
	let failed = 0;

	for (const file of files) {
		let data;

		try {
			data = JSON.parse(readFileSync(file, 'utf8'));
		} catch (e) {
			console.error(`✗ ${file}\n  ${e instanceof Error ? e.message : String(e)}`);
			failed++;
			continue;
		}

		if (validate(data)) {
			process.stdout.write(`✓ ${file}\n`);
		} else {
			console.error([`✗ ${file}`, ...explain(validate.errors)].join('\n'));
			failed++;
		}
	}

	return failed === 0 ? 0 : 1;
}

// Only when run as the command, not when the tests import the helpers above.
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
	process.exit(main(process.argv.slice(2)));
}
