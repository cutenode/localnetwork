import { expect, it } from 'vitest';
import { loadRoute } from '#lib/testing/route-event.ts';
import { demoTrack, tools } from '#lib/tools/index.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const app = { config: { publicUrl: 'https://localnetwork.computer' } };

it('lists the tools with only what the deck shows, under the canonical /tools address', async () => {
	const data = await loadRoute<PageServerData>(load, { app, url: '/tools' });

	expect(data.tools.map((tool) => tool.id)).toEqual(tools.map((tool) => tool.id));
	expect(Object.keys(data.tools[0]).sort()).toEqual(['cues', 'id', 'kind', 'name']);
	expect(data.meta).toMatchObject({ title: 'Tools', url: 'https://localnetwork.computer/tools' });
});

it("places one cue on the demo track for each of the tools' cues, inside the track", async () => {
	const data = await loadRoute<PageServerData>(load, { app, url: '/tools' });

	expect(data.track.durationSecs).toBe(demoTrack.durationSecs);
	expect(data.track.cues).toHaveLength(tools.flatMap((tool) => tool.cues).length);
	expect(data.track.cues.every((cue) => cue >= 0 && cue < demoTrack.durationSecs)).toBe(true);
});
