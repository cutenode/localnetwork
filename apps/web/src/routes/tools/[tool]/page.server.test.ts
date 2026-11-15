import { expect, it } from 'vitest';
import publishing from '../../../../../../lexicons/computer/localnetwork/locallibraryPublishing.json' with { type: 'json' };
import { tools } from '#lib/tools/index.ts';
import { PAT } from '#lib/testing/link-matrix.ts';
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

const locallibraryPage = (viewer: typeof PAT | null = null) =>
	loadRoute<PageServerData>(load, { app, viewer, url: '/tools/locallibrary', params: { tool: 'locallibrary' } });

const publishingHtml = (data: PageServerData) => data.sections.find((section) => section.title === 'Publishing')!.html;

it('says what publishing asks for: the record types the permission set lets it write, and which of them it deletes', async () => {
	const repo = publishing.defs.main.permissions.filter((permission) => permission.resource === 'repo');
	const numbers = ['zero', 'one', 'two', 'three', 'four', 'five', 'six'];

	const deletes = repo
		.filter((permission) => permission.action?.includes('delete'))
		.map(({ collection }) => collection![0]);

	const html = publishingHtml(await locallibraryPage());

	expect(html).toContain(`create and replace ${numbers[repo.length]} kinds of record`);
	expect(deletes).toEqual(['computer.localnetwork.dj.set', 'computer.localnetwork.dj.recording']);
	expect(html).toContain('to delete sets and their recordings');
	expect(html).toContain('to upload a set&#39;s recording');
	expect(html).toContain('href="/tools/locallibrary/docs/account#publishing"');
});

it('links the mix preview, and the profile records are published to', async () => {
	const signedOut = publishingHtml(await locallibraryPage());
	const signedIn = publishingHtml(await locallibraryPage(PAT));

	expect(signedOut).toContain('<a href="/mixes/preview">');
	expect(signedOut).toContain('<a href="/for-djs">your Local Network profile</a>');
	expect(signedIn).toContain('<a href="/@pat.test">your Local Network profile</a>');
	expect(signedIn).not.toContain('/@me');
});

it('lists the changelog last among the guides', async () => {
	const { guides } = await locallibraryPage();

	expect(guides.at(-1)).toEqual({ slug: 'changelog', title: 'Changelog' });
});
