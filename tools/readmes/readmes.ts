// What can be checked about the apps' Mermaid diagrams without drawing them: the Diagrams section of
// each README and the pages in its docs/diagrams/. A diagram's truth is the reader's to check against
// its sources, so each one has to name them and they have to be there: moving a file a diagram
// describes fails here. mermaid.test.ts parses the blocks. A pull request that changes a source but
// not its diagram says why the diagram still holds, which `drift` checks on each one in CI:
//
//   node tools/readmes/cli.ts drift <base-ref> <body-file>
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { posix, resolve } from 'node:path';

// GitHub's anchor for a heading: lower-cased, punctuation dropped, spaces to dashes.
const anchor = (heading: string) =>
	heading
		.toLowerCase()
		.replace(/[^\w\s-]/g, '')
		.trim()
		.replace(/\s+/g, '-');

// GitHub draws a page's diagrams in a light theme and a dark one, so a set colour breaks one of them.
const STYLED = /^\s*(?:style|classDef|linkStyle)\s|%%\{/m;

// GitHub starts every diagram on a page at once, and shows "Unable to render rich display" for those
// it hasn't finished in time. 67 on a page all drew, 83 lost 34 and 116 lost 72, so a page stays well
// under that and a growing one is split.
const MOST_ON_A_PAGE = 40;

/** The Markdown files of the app in `folder` that hold diagrams, relative to it. */
export const diagramFiles = (folder: string) => {
	const pages = resolve(folder, 'docs/diagrams');

	return ['README.md', ...(existsSync(pages) ? readdirSync(pages) : []).map((file) => `docs/diagrams/${file}`)];
};

/** The body of each Mermaid block in `markdown`. */
export const mermaidBlocks = (markdown: string) =>
	[...markdown.matchAll(/^```mermaid\n([\s\S]*?)^```$/gm)].map((match) => match[1]);

/**
 * What's wrong with a README's Diagrams section, or with a page that's all diagrams. `exists` says
 * whether a path relative to the file is there.
 */
export function diagramProblems(markdown: string, exists: (path: string) => boolean): string[] {
	const section = /^## Diagrams\n([\s\S]*?)(?=^## |(?![\s\S]))/m.exec(markdown)?.[1] ?? markdown;
	const headings = new Set([...markdown.matchAll(/^#{1,6} (.+)$/gm)].map((match) => anchor(match[1])));
	const titles = [...section.matchAll(/^#{2,3} (.+)$/gm)].map((match) => anchor(match[1]));
	const targets = [...section.matchAll(/\]\(([^)]+)\)/g)].map((match) => match[1]);
	const indexed = targets.flatMap((target) => (target.startsWith('#') ? [target.slice(1)] : []));
	const files = targets.filter((target) => !/^(?:#|https?:)/.test(target));
	const blocks = [...section.matchAll(/```mermaid\n([\s\S]*?)```\n+(.*)/g)];
	const charts = mermaidBlocks(markdown).length;

	return [
		...(charts > MOST_ON_A_PAGE ? [`${charts} diagrams on a page: GitHub draws about ${MOST_ON_A_PAGE}`] : []),
		...indexed.flatMap((target) => (headings.has(target) ? [] : [`#${target}: no such heading`])),
		...titles.flatMap((title) => (indexed.includes(title) ? [] : [`${title}: not in the index`])),
		...files.flatMap((file) => (exists(file.split('#')[0]) ? [] : [`${file}: no such file`])),
		...blocks.flatMap(([, body, next]) => (next.startsWith('Source: ') ? [] : [`no Source: after ${body}`])),
		...blocks.flatMap(([, body]) => (STYLED.test(body) ? [`sets a colour: ${body}`] : []))
	];
}

/** A diagram: the file it's in, the anchor of the heading above it, and what its Source: line links to. */
export type Diagram = { file: string; heading: string; sources: string[] };

/** The diagrams in `markdown`, the file at `file`, with every path from the repository's root. */
export function diagramsIn(file: string, markdown: string): Diagram[] {
	const found: Diagram[] = [];
	let heading = '';

	for (const [, title, next] of markdown.matchAll(/^#{1,6} (.+)$|^```mermaid\n[\s\S]*?^```\n+(.*)/gm)) {
		if (title !== undefined) {
			heading = anchor(title);
			continue;
		}

		const links = next.startsWith('Source: ')
			? [...next.matchAll(/\]\(([^)#]+)[^)]*\)/g)].map((match) => match[1])
			: [];

		const local = links.filter((target) => !/^https?:/.test(target));
		// A link to a directory may end in a slash; a change anywhere under it changes it.
		const sources = local.map((target) => posix.join(posix.dirname(file), target).replace(/\/$/, ''));
		found.push({ file, heading, sources });
	}

	return found;
}

/**
 * The diagrams a pull request that changes the files in `changed` leaves unchecked, each as a line to
 * print: one of its sources changed, its own file didn't, and the pull request's `body` has no line
 * `diagrams-checked: <file>#<heading>: <why>` for it, or `diagrams-checked: <file>: <why>` for its file.
 */
export function unchecked(diagrams: Diagram[], changed: string[], body: string): string[] {
	const lines = body.split(/\r?\n/).map((line) => line.trim());
	const isNamed = (name: string) => lines.some((line) => line.startsWith(`diagrams-checked: ${name}: `));
	// By heading, which is what a body line names: diagrams under the same one share a line.
	const touched = new Map<string, Set<string>>();

	for (const { file, heading, sources } of diagrams) {
		const name = `${file}#${heading}`;
		const checked = changed.includes(file) || isNamed(file) || isNamed(name);

		for (const source of sources) {
			const isChanged = changed.some((path) => path === source || path.startsWith(`${source}/`));

			if (isChanged && !checked) touched.set(name, (touched.get(name) ?? new Set()).add(source));
		}
	}

	return [...touched].map(([name, sources]) => {
		const file = name.split('#')[0];

		return `${name}: ${[...sources].join(', ')} changed: update the diagram, or add a line \`diagrams-checked: ${name}: <why>\` (or \`diagrams-checked: ${file}: <why>\` for every diagram in the file) to the pull request's body`;
	});
}

/** `unchecked` for every app's diagrams, the changes since `base`, and the body in `bodyFile`. */
function drift(base: string, bodyFile: string): string[] {
	const apps = readdirSync('apps', { withFileTypes: true }).filter((entry) => entry.isDirectory());
	const files = apps.flatMap(({ name }) => diagramFiles(`apps/${name}`).map((file) => `apps/${name}/${file}`));
	const diagrams = files.flatMap((file) => diagramsIn(file, readFileSync(file, 'utf8')));
	// Without renames, so a source moved away counts as changed under the path a diagram names.
	const diff = execFileSync('git', ['diff', '--name-only', '--no-renames', `${base}...HEAD`], { encoding: 'utf8' });

	return unchecked(diagrams, diff.split('\n').filter(Boolean), readFileSync(bodyFile, 'utf8'));
}

/** Runs a command and gives the lines to print and whether it passed. */
export function run([command, base, bodyFile, ...extra]: string[]) {
	if (command !== 'drift' || base === undefined || bodyFile === undefined || extra.length > 0) {
		return { lines: ['usage: cli.ts drift <base-ref> <body-file>'], ok: false };
	}

	const lines = drift(base, bodyFile);

	return { lines, ok: lines.length === 0 };
}
