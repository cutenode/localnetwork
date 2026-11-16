import { error, type RequestEvent } from '@sveltejs/kit';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { act, clientIp, requireViewer } from '#lib/server/http.ts';
import { listed, profileMeta } from '#lib/server/meta.ts';
import { profileRecords } from '#lib/server/meta-records.ts';
import { loadProfilePage, pageAccount } from '#lib/server/profile-page.ts';
import { personProfile } from '#lib/server/structured-data.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async (event) => {
	const { app, viewer } = event.locals;
	// Untracked, so switching tabs (`?tab=`), which the page reads from the URL, doesn't load the profile again.
	const search = event.untrack(() => event.url.search);
	const page = await loadProfilePage(event.locals, event.params.handle, search, clientIp(event));
	const { profile } = page;

	return {
		...page,
		// Streamed like the rest. Only a profile record can be followed, and not by its own account.
		follow:
			profile && profile.did !== viewer?.did
				? unlessOffline(event.locals.log, () => app.follows.state(profile, viewer?.did))
				: undefined,
		meta: await listed(event.locals, profileMeta(app.config.publicUrl, page.account, page.profile, page.name), {
			records: () => profileRecords(page.profile),
			jsonLd: page.profile && personProfile(page.profile, page.name)
		})
	};
};

/** The signed-in viewer, and the profile the page is about. A thrown 404 when the account has no profile. */
async function followTarget(event: RequestEvent<{ handle: string }>) {
	const viewer = requireViewer(event.locals, event.url);
	const account = await pageAccount(event.locals, event.params.handle, '', clientIp(event));
	const profile = await event.locals.app.profiles.get(account.did);

	if (!profile) error(404, 'Profile not found');

	return { viewer, profile };
}

export const actions: Actions = {
	follow: async (event) => {
		const { viewer, profile } = await followTarget(event);

		return act(async () => {
			await event.locals.app.follows.follow(viewer, profile);

			return { followed: true };
		});
	},
	unfollow: async (event) => {
		const { viewer, profile } = await followTarget(event);

		return act(async () => {
			await event.locals.app.follows.unfollow(viewer, profile);

			return { followed: false };
		});
	}
};
