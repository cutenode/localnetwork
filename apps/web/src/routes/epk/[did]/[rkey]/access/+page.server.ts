import { error } from '@sveltejs/kit';
import { UserError } from '#lib/server/core.ts';
import { loadEpk } from '#lib/server/epk-page.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

async function owned(locals: App.Locals, url: URL, did: string, rkey: string) {
	const viewer = requireViewer(locals, url);

	if (viewer.did !== did) error(403, 'Only the owner of this press kit can see who has access');

	return { viewer, ...(await loadEpk(locals.app.epks, did, rkey)) };
}

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const { viewer, epk, project } = await owned(locals, url, params.did, params.rkey);

	await locals.app.epks.settle(viewer);
	const [requests, grants] = await Promise.all([locals.app.epks.requests(epk), locals.app.epks.grants(epk)]);

	return { epk, project, requests, grants };
};

export const actions: Actions = {
	approve: async ({ request, locals, url, params }) => {
		const { viewer, epk } = await owned(locals, url, params.did, params.rkey);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.decide(viewer, epk, text(form, 'request', 2000), 'approve');

			return { success: 'Approved. They can see the whole press kit now.' };
		});
	},
	decline: async ({ request, locals, url, params }) => {
		const { viewer, epk } = await owned(locals, url, params.did, params.rkey);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.decide(viewer, epk, text(form, 'request', 2000), 'decline');

			return { success: 'Declined.' };
		});
	},
	revoke: async ({ request, locals, url, params }) => {
		const { viewer, epk } = await owned(locals, url, params.did, params.rkey);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.revoke(viewer, epk, text(form, 'subject', 2000));

			return { success: 'Access removed.' };
		});
	},
	invite: async ({ request, locals, url, params }) => {
		const { viewer, epk } = await owned(locals, url, params.did, params.rkey);
		const form = await request.formData();
		const handle = text(form, 'handle', 300);

		return act(
			async () => {
				if (!handle) throw new UserError('Enter a handle');
				await locals.app.epks.invite(viewer, epk, handle);

				return { success: `Added @${handle.replace(/^@/, '')}.` };
			},
			{ handle }
		);
	}
};
