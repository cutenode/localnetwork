import { sectionsOf } from '#lib/server/forms/profile.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ locals, url, params }) => {
	const viewer = requireViewer(locals, url);
	// Press kits this account made before the root held them move to the root now, without holding the page up; they
	// stay listed and readable where they are until then.
	void locals.app.epks
		.migrateLegacy(viewer)
		.catch((error) => locals.log.warn({ err: error }, 'could not move legacy press kits'));

	// Opening a booking thread reads what the inbox said about it. That happens here rather than in the thread's own
	// load, which runs alongside this one, so the badge on the thread's page already leaves it out.
	const thread = params.skey ? await locals.app.bookings.bySkey(params.skey) : null;

	if (thread) await locals.app.notifications.markReadBySubject(viewer.did, thread);

	return {
		// Streamed, so the dashboard shows while the directory finds them; null when it's offline.
		projects: unlessOffline(locals.log, () => locals.app.projects.managedBy(viewer.did)),
		// Which sections the account's profile has on, which decide the sidebar; null without a profile, or while the
		// directory is offline, when the sidebar shows everything.
		sections: unlessOffline(locals.log, async () => {
			const profile = await locals.app.profiles.get(viewer.did);

			return profile && sectionsOf(profile.record);
		}).then((sections) => sections ?? null),
		unread: await locals.app.notifications.unreadCount(viewer.did)
	};
};
