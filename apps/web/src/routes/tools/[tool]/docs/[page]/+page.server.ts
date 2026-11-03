import { error } from '@sveltejs/kit';
import { pageMeta } from '#lib/server/meta.ts';
import { renderMarkdown } from '#lib/server/tools/markdown.ts';
import { findTool } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

/** A guide with this many sections or more opens on a list of them. */
const CONTENTS_FROM = 4;

/** A rendered heading's words: marked wraps code in tags and escapes only these five, `&` first. */
const headingText = (html: string) =>
	html
		.replace(/<[^>]*>/g, '')
		.replaceAll('&lt;', '<')
		.replaceAll('&gt;', '>')
		.replaceAll('&quot;', '"')
		.replaceAll('&#39;', "'")
		.replaceAll('&amp;', '&');

export const load: PageServerLoad = ({ params, locals }) => {
	const tool = findTool(params.tool);
	const doc = tool?.docs.find((candidate) => candidate.slug === params.page);

	if (!tool || !doc) error(404, "There's no guide by that name.");
	const html = renderMarkdown(doc.markdown, tool, doc.path);

	const sections = [...html.matchAll(/<h2 id="([^"]+)">(.*?)<\/h2>/g)].map(([, id, inner]) => ({
		id,
		title: headingText(inner)
	}));

	return {
		tool: { id: tool.id, name: tool.name },
		doc: { slug: doc.slug, title: doc.title, path: doc.path, html },
		contents: sections.length >= CONTENTS_FROM ? sections : [],
		guides: tool.docs.map(({ slug, title }) => ({ slug, title })),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: `${doc.title} · ${tool.name}`,
			description: `${tool.name} guide: ${doc.title}.`,
			path: `/tools/${tool.id}/docs/${doc.slug}`
		})
	};
};
