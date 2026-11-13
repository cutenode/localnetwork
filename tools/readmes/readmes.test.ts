import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { diagramFiles, diagramProblems, diagramsIn, run, unchecked } from './readmes.ts';

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

	it('reads a page that is all diagrams, with no Diagrams section', () => {
		const page = `# app: one area\n\n- [One](#one)\n\n## One\n\n${block()}`;

		expect(diagramProblems(page, (path) => path === 'a.ts')).toEqual([]);
		expect(diagramProblems(`# app: one area\n\n## One\n\n${block()}`, () => true)).toEqual(['one: not in the index']);
	});

	it('fails a page with more diagrams than GitHub finishes drawing', () => {
		const within = `- [One](#one)\n\n## One\n\n${block().repeat(40)}`;

		expect(diagramProblems(within, () => true)).toEqual([]);
		expect(diagramProblems(`${within}${block()}`, () => true)).toEqual([
			'41 diagrams on a page: GitHub draws about 40'
		]);
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

	it.each(['locallibrary', 'localtoolbox', 'web'])("holds for apps/%s's README and diagram pages", (app) => {
		const folder = resolve(repo, 'apps', app);

		expect(readFileSync(resolve(folder, 'README.md'), 'utf8')).toContain('\n## Diagrams\n');

		for (const file of diagramFiles(folder)) {
			const here = dirname(resolve(folder, file));

			expect(
				diagramProblems(readFileSync(resolve(folder, file), 'utf8'), (path) => existsSync(resolve(here, path))),
				file
			).toEqual([]);
		}
	});
});

// A diagram page two levels into an app, as apps/*/docs/diagrams/*.md are.
const PAGE = 'apps/app/docs/diagrams/page.md';

const page = [
	'# app: one area',
	'## Sign in',
	block(undefined, 'Source: [`a.ts`](../../src/a.ts#L3), [`lib/`](../../src/lib/), [spec](https://example.com/a.ts)'),
	'## The store',
	block(undefined, 'Source: [`store.rs`](../../../../crates/core/src/store.rs)')
].join('\n\n');

const diagrams = diagramsIn(PAGE, page);

const reported = (file: string, heading: string, paths: string) =>
	`${file}#${heading}: ${paths} changed: update the diagram, or add a line \`diagrams-checked: ${file}#${heading}: <why>\` (or \`diagrams-checked: ${file}: <why>\` for every diagram in the file) to the pull request's body`;

const signIn = (paths: string) => reported(PAGE, 'sign-in', paths);

describe('diagramsIn', () => {
	it('gives each diagram its heading and its sources from the repository root', () => {
		expect(diagrams).toEqual([
			{ file: PAGE, heading: 'sign-in', sources: ['apps/app/src/a.ts', 'apps/app/src/lib'] },
			{ file: PAGE, heading: 'the-store', sources: ['crates/core/src/store.rs'] }
		]);
	});

	it('gives a diagram with no Source: line no sources', () => {
		expect(diagramsIn(PAGE, `## One\n\n${block(undefined, 'Nothing here.')}`)).toEqual([
			{ file: PAGE, heading: 'one', sources: [] }
		]);
	});
});

describe('unchecked', () => {
	it('reports a diagram whose source changed, with the sources that did', () => {
		expect(unchecked(diagrams, ['apps/app/src/a.ts', 'apps/app/src/lib/b.ts'], '')).toEqual([
			signIn('apps/app/src/a.ts, apps/app/src/lib')
		]);
	});

	it('takes a change under a source directory as a change to it, and not one beside it', () => {
		expect(unchecked(diagrams, ['apps/app/src/lib/deep/c.ts'], '')).toEqual([signIn('apps/app/src/lib')]);
		expect(unchecked(diagrams, ['apps/app/src/library.ts'], '')).toEqual([]);
	});

	it('resolves a source outside the app through ../', () => {
		expect(unchecked(diagrams, ['crates/core/src/store.rs'], '')).toEqual([
			reported(PAGE, 'the-store', 'crates/core/src/store.rs')
		]);
	});

	it('passes a diagram the body says why is still right, and only that one', () => {
		const changed = ['apps/app/src/a.ts', 'crates/core/src/store.rs'];
		const body = `## What and why\n\n  diagrams-checked: ${PAGE}#the-store: a rename inside the function\r\n`;

		expect(unchecked(diagrams, changed, body)).toEqual([signIn('apps/app/src/a.ts')]);
		expect(unchecked(diagrams, changed, `${body}diagrams-checked: ${PAGE}#sign-in: only a log line\n`)).toEqual([]);
	});

	it("passes every diagram in a file the body names, and no other file's", () => {
		const changed = ['apps/app/src/a.ts', 'crates/core/src/store.rs'];
		const other = { file: 'apps/app/README.md', heading: 'one', sources: ['apps/app/src/a.ts'] };
		const body = `diagrams-checked: ${PAGE}: the page only describes the exported names, which stayed\n`;

		expect(unchecked([...diagrams, other], changed, body)).toEqual([
			reported('apps/app/README.md', 'one', 'apps/app/src/a.ts')
		]);
		expect(unchecked(diagrams, changed, `diagrams-checked: ${PAGE}: \n`)).toHaveLength(2);
	});

	it('wants a reason after the diagram in the body', () => {
		expect(unchecked(diagrams, ['apps/app/src/a.ts'], `diagrams-checked: ${PAGE}#sign-in: \n`)).toEqual([
			signIn('apps/app/src/a.ts')
		]);
	});

	it("passes every diagram in a file the pull request changed, and one that touches none's sources", () => {
		expect(unchecked(diagrams, ['apps/app/src/a.ts', 'crates/core/src/store.rs', PAGE], '')).toEqual([]);
		expect(unchecked(diagrams, ['apps/app/src/b.ts', 'README.md'], '')).toEqual([]);
	});

	it('reports the diagrams under one heading as one, with each changed source once', () => {
		const second = { ...diagrams[0], sources: ['apps/app/src/a.ts', 'apps/app/src/c.ts'] };
		const changed = ['apps/app/src/a.ts', 'apps/app/src/c.ts'];

		expect(unchecked([diagrams[0], second], changed, '')).toEqual([signIn('apps/app/src/a.ts, apps/app/src/c.ts')]);
	});
});

describe('run', () => {
	const start = process.cwd();
	let dir: string;

	const git = (...args: string[]) =>
		execFileSync('git', ['-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@t', ...args]);

	const commit = (message: string) => {
		git('add', '.');
		git('commit', '-qm', message);
	};

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'readmes-'));
		process.chdir(dir);
		mkdirSync('apps/app/docs/diagrams', { recursive: true });
		mkdirSync('apps/app/src');
		mkdirSync('apps/other');
		writeFileSync('apps/app/docs/diagrams/page.md', page);
		writeFileSync('apps/app/README.md', readme(block(undefined, 'Source: [`a.ts`](src/a.ts)')));
		writeFileSync('apps/app/src/a.ts', '');
		// An app with a README and no diagram pages, and a file beside the apps.
		writeFileSync('apps/other/README.md', '# other\n');
		writeFileSync('apps/.keep', '');
		writeFileSync('body.md', '');
		git('init', '-q');
		commit('base');
	});
	afterEach(() => {
		process.chdir(start);
		rmSync(dir, { recursive: true });
	});

	it('reports the diagrams whose sources changed since the base', () => {
		writeFileSync('apps/app/src/a.ts', 'export {};\n');
		commit('change');

		expect(run(['drift', 'HEAD~1', 'body.md'])).toEqual({
			lines: [reported('apps/app/README.md', 'one', 'apps/app/src/a.ts'), signIn('apps/app/src/a.ts')],
			ok: false
		});
	});

	it('passes once each diagram is updated or checked in the body', () => {
		writeFileSync('apps/app/src/a.ts', 'export {};\n');
		writeFileSync('apps/app/README.md', readme(block('flowchart TD\n  A --> C\n', 'Source: [`a.ts`](src/a.ts)')));
		commit('change');
		writeFileSync('body.md', `diagrams-checked: ${PAGE}#sign-in: a.ts only exports now\n`);

		expect(run(['drift', 'HEAD~1', 'body.md'])).toEqual({ lines: [], ok: true });
	});

	it.each([[[]], [['drift', 'HEAD']], [['check', 'HEAD', 'body.md']], [['drift', 'HEAD', 'body.md', 'extra']]])(
		'prints its usage for %j',
		(args) => {
			expect(run(args)).toEqual({ lines: ['usage: cli.ts drift <base-ref> <body-file>'], ok: false });
		}
	);
});
