import type { ToolDoc } from './types.ts';

/**
 * A tool's guides from its README and `docs/*.md`, given as an eager `?raw` `import.meta.glob`
 * of them and the tool's folder in the repo. The README comes first as the overview, then the
 * guides in the order the README first links them, then any it doesn't link, by title.
 */
export function loadDocs(repoDir: string, files: Record<string, string>): ToolDoc[] {
	const docs = Object.entries(files).map(([file, source]): ToolDoc => {
		const local = /(?:^|\/)(README\.md|docs\/[^/]+\.md)$/.exec(file)?.[1];

		if (!local) throw new Error(`${file} isn't a README or a file in docs/`);
		const isReadme = local === 'README.md';
		const heading = /^# (.+)\n/.exec(source);

		return {
			slug: isReadme ? 'readme' : local.slice('docs/'.length, -'.md'.length),
			path: `${repoDir}/${local}`,
			title: isReadme ? 'Overview' : (heading?.[1].trim() ?? local),
			markdown: heading ? source.slice(heading[0].length) : source
		};
	});

	const readme = docs.find((doc) => doc.slug === 'readme');
	const linked = [...(readme?.markdown ?? '').matchAll(/\]\((?:\.\/)?docs\/([^/)#]+)\.md/g)].map((match) => match[1]);

	const rank = (doc: ToolDoc) =>
		doc === readme ? -1 : linked.includes(doc.slug) ? linked.indexOf(doc.slug) : linked.length;

	return docs.toSorted((a, b) => rank(a) - rank(b) || a.title.localeCompare(b.title));
}
