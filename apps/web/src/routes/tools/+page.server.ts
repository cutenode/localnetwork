import { lookOf } from '#lib/components/tools/HotCues.svelte';
import { slimCapture } from '#lib/components/tools/sources.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { demoTrack, tools } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({
	// In the order the page shows them. Each is shown its own way, by where it works (`look`): a tool for
	// rekordbox's drives as hot cues on a track, the one that works on SoundCloud as its own menu.
	tools: tools.map(({ id, name, pitch, cues, docs }) => ({
		id,
		name,
		pitch,
		look: lookOf(id),
		// Where its Guides button goes: the first of its guides, which leads the rest.
		guide: docs[0] ? `/tools/${id}/docs/${docs[0].slug}` : `/tools/${id}`,
		cues: cues.map((cue) => ({ ...cue, capture: slimCapture(cue.capture) }))
	})),
	track: demoTrack,
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'Tools',
		description:
			'Two free tools for DJs: a terminal app for your rekordbox USB drives, and a Chrome extension for digging on SoundCloud.',
		path: '/tools'
	})
});
