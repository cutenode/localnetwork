import { error } from '@sveltejs/kit';
import { lookOf } from '#lib/components/tools/HotCues.svelte';
import { slimCapture } from '#lib/components/tools/sources.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { renderSections } from '#lib/server/tools/markdown.ts';
import { demoTrack, findTool, tools } from '#lib/tools/index.ts';
import type { Tool } from '#lib/tools/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ params, locals }) => {
	const tool = findTool(params.tool);

	if (!tool) error(404, "There's no tool by that name.");
	const { id, name, kind, tagline, summary, install, status } = tool;

	// Typed as the tool's own fields, so `status` stays optional in the page's data.
	const shown: Pick<Tool, 'id' | 'name' | 'kind' | 'tagline' | 'summary' | 'install' | 'status'> = {
		id,
		name,
		kind,
		tagline,
		summary,
		install,
		status
	};

	return {
		tool: { ...shown, cues: tool.cues.map((cue) => ({ ...cue, capture: slimCapture(cue.capture) })) },
		track: demoTrack,
		look: lookOf(id),
		others: tools
			.filter((other) => other.id !== id)
			.map((other) => ({ id: other.id, name: other.name, tagline: other.tagline })),
		sections: renderSections(tool),
		guides: tool.docs.map(({ slug, title }) => ({ slug, title })),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: name,
			description: `${name}: ${tagline}`,
			path: `/tools/${id}`
		})
	};
};
