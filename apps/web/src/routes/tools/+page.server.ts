import { pageMeta } from '#lib/server/meta.ts';
import { cuePoints } from '#lib/tools/cues.ts';
import { demoTrack, tools } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

// Where each of the tools' cues sits on the demo track, in seconds, in the order the tools come. Worked
// out here rather than in the browser: a module both Tools pages import joins the chunk every page loads.
const cuePositions = cuePoints(tools.flatMap((tool) => tool.cues).length, demoTrack.durationSecs, demoTrack.phrases);

export const load: PageServerLoad = ({ locals }) => ({
	tools: tools.map(({ id, name, kind, cues }) => ({ id, name, kind, cues })),
	track: { ...demoTrack, cues: cuePositions },
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'Tools',
		description:
			'Tools for DJs from Local Network. Each one stands on its own and signs in with your Atmosphere account.',
		path: '/tools'
	})
});
