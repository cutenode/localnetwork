// What can be checked about an app README's Mermaid diagrams without drawing them. A diagram's
// truth is the reader's to check against its sources, so each one has to name them and they have
// to be there: moving a file a diagram describes fails here. mermaid.test.ts parses the blocks.

// GitHub's anchor for a heading: lower-cased, punctuation dropped, spaces to dashes.
const anchor = (heading: string) =>
	heading
		.toLowerCase()
		.replace(/[^\w\s-]/g, '')
		.trim()
		.replace(/\s+/g, '-');

// GitHub draws a README's diagrams in a light theme and a dark one, so a set colour breaks one of them.
const STYLED = /^\s*(?:style|classDef|linkStyle)\s|%%\{/m;

/** The body of each Mermaid block in `readme`. */
export const mermaidBlocks = (readme: string) =>
	[...readme.matchAll(/^```mermaid\n([\s\S]*?)^```$/gm)].map((match) => match[1]);

/** What's wrong with `readme`'s Diagrams section; `exists` says whether a path relative to it is a file. */
export function diagramProblems(readme: string, exists: (path: string) => boolean): string[] {
	const section = /^## Diagrams\n([\s\S]*?)(?=^## |(?![\s\S]))/m.exec(readme)?.[1];

	if (section === undefined) return ['no ## Diagrams section'];
	const headings = new Set([...readme.matchAll(/^#{1,6} (.+)$/gm)].map((match) => anchor(match[1])));
	const titles = [...section.matchAll(/^### (.+)$/gm)].map((match) => anchor(match[1]));
	const targets = [...section.matchAll(/\]\(([^)]+)\)/g)].map((match) => match[1]);
	const indexed = targets.flatMap((target) => (target.startsWith('#') ? [target.slice(1)] : []));
	const files = targets.filter((target) => !/^(?:#|https?:)/.test(target));
	const blocks = [...section.matchAll(/```mermaid\n([\s\S]*?)```\n+(.*)/g)];

	return [
		...indexed.flatMap((target) => (headings.has(target) ? [] : [`#${target}: no such heading`])),
		...titles.flatMap((title) => (indexed.includes(title) ? [] : [`${title}: not in the index`])),
		...files.flatMap((file) => (exists(file.split('#')[0]) ? [] : [`${file}: no such file`])),
		...blocks.flatMap(([, body, next]) => (next.startsWith('Source: ') ? [] : [`no Source: after ${body}`])),
		...blocks.flatMap(([, body]) => (STYLED.test(body) ? [`sets a colour: ${body}`] : []))
	];
}
