import type { RequesterAbout } from '#lib/epk.ts';
import { UserError } from '#lib/server/core.ts';
import { managedKit } from '#lib/server/epk-page.ts';
import { act, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const { kit, legacyMaker } = await managedKit(locals, url, params);
	const { epk } = kit;
	const [requests, grants] = await Promise.all([locals.app.epks.requests(epk), locals.app.epks.grants(epk)]);
	const askers = requests.map((request) => request.requester.did);
	const [profiles, venues] = await Promise.all([locals.app.profiles.many(askers), locals.app.venues.runBy(askers)]);

	const about = (did: string): RequesterAbout => {
		const record = profiles.get(did)?.record;

		return { promoter: record?.promoter ?? null, performer: record?.performer ?? null, venues: venues.get(did) ?? [] };
	};

	return {
		epk,
		legacyMaker,
		requests: requests.map((request) => ({ ...request, about: about(request.requester.did) })),
		grants
	};
};

export const actions: Actions = {
	approve: async ({ request, locals, url, params }) => {
		const { viewer, kit } = await managedKit(locals, url, params);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.decide(viewer, kit, text(form, 'request', 2000), 'approve');

			return { success: 'Approved. They can see the whole press kit now.' };
		});
	},
	decline: async ({ request, locals, url, params }) => {
		const { viewer, kit } = await managedKit(locals, url, params);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.decide(viewer, kit, text(form, 'request', 2000), 'decline');

			return { success: 'Declined.' };
		});
	},
	revoke: async ({ request, locals, url, params }) => {
		const { viewer, kit } = await managedKit(locals, url, params);
		const form = await request.formData();

		return act(async () => {
			await locals.app.epks.revoke(viewer, kit, text(form, 'subject', 2000));

			return { success: 'Access removed.' };
		});
	},
	invite: async ({ request, locals, url, params }) => {
		const { viewer, kit } = await managedKit(locals, url, params);
		const form = await request.formData();
		const handle = text(form, 'handle', 300);

		return act(
			async () => {
				if (!handle) throw new UserError('Enter a handle');
				await locals.app.epks.invite(viewer, kit, handle);

				return { success: `Added @${handle.replace(/^@/, '')}.` };
			},
			{ handle }
		);
	}
};
