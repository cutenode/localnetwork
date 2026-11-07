import { expect, it } from 'vitest';
import { tools } from '#lib/tools/index.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const app = { config: { publicUrl: 'https://localnetwork.computer' } };

it("opens a tool's page with its cues, guides and the other tools", async () => {
	const tool = tools.find((candidate) => candidate.id === 'locallibrary')!;

	const data = await loadRoute<PageServerData>(load, {
		app,
		url: '/tools/locallibrary',
		params: { tool: 'locallibrary' }
	});

	expect(data.tool).toMatchObject({ id: 'locallibrary', name: tool.name, tagline: tool.tagline });
	expect(data.tool.cues).toHaveLength(tool.cues.length);
	expect(data.guides).toEqual(tool.docs.map(({ slug, title }) => ({ slug, title })));
	expect(data.others).toEqual([{ id: 'localtoolbox', name: expect.any(String), tagline: expect.any(String) }]);
	expect(data.meta).toMatchObject({ title: tool.name, url: 'https://localnetwork.computer/tools/locallibrary' });
});

it("answers 404 for a tool that doesn't exist", async () => {
	await expect(callRoute(load, { app, url: '/tools/nope', params: { tool: 'nope' } })).rejects.toMatchObject({
		status: 404
	});
});
