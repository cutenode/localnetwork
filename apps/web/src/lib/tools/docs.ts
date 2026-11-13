import type { ToolDoc } from './types.ts';

/**
 * A tool's guides from its README and `docs/*.md`, given as an eager `?raw` `import.meta.glob`
 * of them and the tool's folder in the repo. The overview comes first, then the guides in the
 * order it first links them, then any it doesn't link, by title. The overview is the README, or
 * `docs/overview.md` for a tool whose glob leaves out a README written for developers. Any file's
 * `## Diagrams` section stays on GitHub: it's for developers, and only GitHub draws Mermaid.
 */
export function loadDocs(repoDir: string, files: Record<string, string>): ToolDoc[] {
	const docs = Object.entries(files).map(([file, contents]): ToolDoc => {
		const local = /(?:^|\/)(README\.md|docs\/[^/]+\.md)$/.exec(file)?.[1];
		const source = contents.replace(/^## Diagrams\n[\s\S]*?(?=^## |(?![\s\S]))/m, '');

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

	const overview = docs.find((doc) => doc.slug === 'readme') ?? docs.find((doc) => doc.slug === 'overview');

	// The README links `docs/<slug>.md`, docs/overview.md its neighbours as `<slug>.md`.
	const linked = [...(overview?.markdown ?? '').matchAll(/\]\((?:\.\/)?(?:docs\/)?([^/)#]+)\.md/g)].map(
		(match) => match[1]
	);

	const rank = (doc: ToolDoc) =>
		doc === overview ? -1 : linked.includes(doc.slug) ? linked.indexOf(doc.slug) : linked.length;

	return docs.toSorted((a, b) => rank(a) - rank(b) || a.title.localeCompare(b.title));
}
