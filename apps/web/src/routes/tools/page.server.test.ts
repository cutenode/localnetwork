import { expect, it } from 'vitest';
import { loadRoute } from '#lib/testing/route-event.ts';
import { demoTrack, tools } from '#lib/tools/index.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const app = { config: { publicUrl: 'https://localnetwork.computer' } };

it('lists the tools in order with only what their sections show, under the canonical /tools address', async () => {
	const data = await loadRoute<PageServerData>(load, { app, url: '/tools' });

	expect(data.tools.map((tool) => tool.id)).toEqual(tools.map((tool) => tool.id));
	expect(Object.keys(data.tools[0]).sort()).toEqual(['cues', 'guide', 'id', 'look', 'name', 'pitch']);
	expect(data.meta).toMatchObject({ title: 'Tools', url: 'https://localnetwork.computer/tools' });
});

it('shows locallibrary as hot cues and localtoolbox as its menu, each with its Guides button on its first guide', async () => {
	const data = await loadRoute<PageServerData>(load, { app, url: '/tools' });

	expect(data.tools.map(({ id, look, guide }) => ({ id, look, guide }))).toEqual([
		{ id: 'locallibrary', look: 'rekordbox', guide: '/tools/locallibrary/docs/readme' },
		{ id: 'localtoolbox', look: 'soundcloud', guide: '/tools/localtoolbox/docs/overview' }
	]);
	expect(data.track).toEqual(demoTrack);
});
