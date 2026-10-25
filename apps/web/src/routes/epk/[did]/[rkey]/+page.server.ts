import { loadEpk } from '#lib/server/epk-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const { app, viewer } = locals;
	const { epk, project } = await loadEpk(app.epks, params.did, params.rkey);
	const isOwner = viewer?.did === epk.did;

	if (isOwner) await app.epks.settle(viewer);
	const access = await app.epks.accessState(epk, viewer);
	const canSee = access.kind === 'owner' || access.kind === 'granted';

	const content = canSee ? await app.epks.content(epk) : null;

	return {
		epk,
		project,
		access,
		content,
		// The project's mixes with tracklists, by URL, so the press kit can link each one's tracklist.
		tracklists: Object.fromEntries(
			(project.profile.mixes ?? []).flatMap((mix, index) => (mix.tracklist ? [[mix.url, index + 1]] : []))
		),
		dj: app.dj.activity([epk.owner])
	};
};
