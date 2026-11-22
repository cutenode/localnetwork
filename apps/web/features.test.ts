// The features the README's Features section links, one file each in docs/features/<section>/.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';

const FEATURES = join(import.meta.dirname, 'docs/features');

it("links every section's folder from the README", () => {
	const readme = readFileSync(join(import.meta.dirname, 'README.md'), 'utf8');

	const sections = readdirSync(FEATURES, { withFileTypes: true })
		.filter((entry) => entry.isDirectory())
		.map((entry) => entry.name);

	expect(sections.filter((section) => !readme.includes(`](docs/features/${section}/)`))).toEqual([]);
});

it('gives every feature a title', () => {
	const files = readdirSync(FEATURES, { recursive: true, encoding: 'utf8' }).filter((file) => file.endsWith('.md'));

	expect(files.filter((file) => !readFileSync(join(FEATURES, file), 'utf8').startsWith('# '))).toEqual([]);
});
