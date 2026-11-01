import { act, requireAdmin, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireAdmin(locals, url);

	const verifications = await locals.app.verifications.list().catch((error) => {
		locals.log.warn({ err: error }, 'could not list verifications');

		return null;
	});

	const subjects = [...new Set(verifications?.map((verification) => verification.subject))];

	return {
		verifications,
		// Streamed one by one, so the table shows at once and each status fills in as HappyView answers for its account.
		live: Object.fromEntries(subjects.map((did) => [did, locals.app.verifications.live(did)])),
		subject: url.searchParams.get('subject') ?? ''
	};
};

export const actions: Actions = {
	grant: async ({ locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const form = await request.formData();

		return act(async () => {
			const did = await locals.app.identity.resolveIdentifier(text(form, 'subject', 300));
			const { handle, displayName } = await locals.app.verifications.grant(viewer, did);

			return { success: `Verified @${handle} as “${displayName}”` };
		});
	},
	revoke: async ({ locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const form = await request.formData();

		return act(async () => {
			await locals.app.verifications.revoke(viewer, text(form, 'did', 300));

			return { success: 'Verification removed' };
		});
	}
};
