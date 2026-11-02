import { error } from '@sveltejs/kit';
import { pageMeta } from '#lib/server/meta.ts';
import { renderMarkdown } from '#lib/server/tools/markdown.ts';
import { demoTrack, findTool } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ params, locals }) => {
	const tool = findTool(params.tool);

	if (!tool) error(404, "There's no tool by that name.");
	const { id, name, kind, tagline, summary, cues, install } = tool;
	// Links in a section resolve like links in the README.
	const readme = tool.docs.find((doc) => doc.slug === 'readme')?.path ?? '';

	return {
		tool: { id, name, kind, tagline, summary, cues, install },
		track: demoTrack,
		sections: (tool.sections ?? []).map((section) => ({
			title: section.title,
			html: renderMarkdown(section.markdown, tool, readme)
		})),
		guides: tool.docs.map(({ slug, title }) => ({ slug, title })),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: name,
			description: `${name}: ${tagline}`,
			path: `/tools/${id}`
		})
	};
};
