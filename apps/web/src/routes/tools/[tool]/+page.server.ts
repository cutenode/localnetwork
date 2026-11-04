import { error } from '@sveltejs/kit';
import { pageMeta } from '#lib/server/meta.ts';
import { renderSections } from '#lib/server/tools/markdown.ts';
import { demoTrack, findTool } from '#lib/tools/index.ts';
import type { Tool } from '#lib/tools/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ params, locals }) => {
	const tool = findTool(params.tool);

	if (!tool) error(404, "There's no tool by that name.");
	const { id, name, kind, tagline, summary, cues, install, status } = tool;

	// Typed as the tool's own fields, so `status` stays optional in the page's data.
	const shown: Pick<Tool, 'id' | 'name' | 'kind' | 'tagline' | 'summary' | 'cues' | 'install' | 'status'> = {
		id,
		name,
		kind,
		tagline,
		summary,
		cues,
		install,
		status
	};

	return {
		tool: shown,
		track: demoTrack,
		sections: renderSections(tool),
		guides: tool.docs.map(({ slug, title }) => ({ slug, title })),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: name,
			description: `${name}: ${tagline}`,
			path: `/tools/${id}`
		})
	};
};
