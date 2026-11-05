import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { diagramProblems } from './readmes.ts';

const repo = resolve(import.meta.dirname, '../..');

const block = (body = 'flowchart TD\n  A --> B\n', after = 'Source: [`a.ts`](a.ts)') =>
	`\`\`\`mermaid\n${body}\`\`\`\n\n${after}\n`;

const readme = (diagram = block()) =>
	`# app\n\n## Use\n\n## Diagrams\n\n- [One](#one)\n\n### One\n\nIt shows one.\n\n${diagram}\n## Credits\n`;

const noFiles = () => false;

describe('diagramProblems', () => {
	it('passes a section whose index, sources and blocks all hold', () => {
		expect(diagramProblems(readme(), (path) => path === 'a.ts')).toEqual([]);
	});

	it('accepts a link to a line of a file, a web address, and an index entry for a heading elsewhere', () => {
		const links = block(undefined, 'Source: [`a.ts`](a.ts#L1), [spec](https://example.com), [Use](#use)');

		expect(diagramProblems(readme(links), (path) => path === 'a.ts')).toEqual([]);
	});

	it('fails a README with no section', () => {
		expect(diagramProblems('# app\n', noFiles)).toEqual(['no ## Diagrams section']);
	});

	it('fails a source that is not there', () => {
		expect(diagramProblems(readme(), noFiles)).toEqual(['a.ts: no such file']);
	});

	it('fails an index entry with no heading, and a heading with no index entry', () => {
		const section = `## Diagrams\n\n- [Two](#two)\n\n### One\n\n${block()}`;

		expect(diagramProblems(section, () => true)).toEqual(['#two: no such heading', 'one: not in the index']);
	});

	it('fails a block with no sources after it', () => {
		expect(diagramProblems(readme(block(undefined, 'Nothing here.')), () => true)).toEqual([
			'no Source: after flowchart TD\n  A --> B\n'
		]);
	});

	it.each(['  style A fill:#fff\n', '  classDef a fill:#fff\n', '  linkStyle 0 stroke:#fff\n', '%%{init: {}}%%\n'])(
		'fails a block that sets a colour: %s',
		(line) => {
			const body = `flowchart TD\n${line}`;

			expect(diagramProblems(readme(block(body)), () => true)).toEqual([`sets a colour: ${body}`]);
		}
	);

	it.each(['locallibrary', 'localtoolbox', 'web'])("holds for apps/%s's README", (app) => {
		const folder = resolve(repo, 'apps', app);

		expect(
			diagramProblems(readFileSync(resolve(folder, 'README.md'), 'utf8'), (path) => existsSync(resolve(folder, path)))
		).toEqual([]);
	});
});
