// What can be checked about the apps' Mermaid diagrams without drawing them: the Diagrams section of
// each README and the pages in its docs/diagrams/. A diagram's truth is the reader's to check against
// its sources, so each one has to name them and they have to be there: moving a file a diagram
// describes fails here. mermaid.test.ts parses the blocks.
import { readdirSync } from 'node:fs';
import { resolve } from 'node:path';

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
export const diagramFiles = (folder: string) => [
	'README.md',
	...readdirSync(resolve(folder, 'docs/diagrams')).map((file) => `docs/diagrams/${file}`)
];

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
