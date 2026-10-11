import { error } from '@sveltejs/kit';
import { renderMarkdown } from '#lib/server/tools/markdown.ts';
import { findTool } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ params }) => {
	const tool = findTool(params.tool);
	const doc = tool?.docs.find((candidate) => candidate.slug === params.page);

	if (!tool || !doc) error(404, "There's no guide by that name.");

	return {
		tool: { id: tool.id, name: tool.name },
		doc: { slug: doc.slug, title: doc.title, path: doc.path, html: renderMarkdown(doc.markdown, tool, doc.path) },
		guides: tool.docs.map(({ slug, title }) => ({ slug, title }))
	};
};
