import { sectionsOf } from '#lib/server/forms/profile.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { accessWaiting, needsCount, needsYou, soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ locals, url, params }) => {
	const viewer = requireViewer(locals, url);
	// Press kits this account made before the root held them move to the root now, without holding the page up; they
	// stay listed and readable where they are until then.
	void locals.app.epks
		.migrateLegacy(viewer)
		.catch((error) => locals.log.warn('could not move legacy press kits', { err: error }));

	// Opening a booking thread reads what the inbox said about it. That happens here rather than in the thread's own
	// load, which runs alongside this one, so the badge on the thread's page already leaves it out.
	const thread = params.skey ? await locals.app.bookings.bySkey(params.skey) : null;

	if (thread) await locals.app.notifications.markReadBySubject(viewer.did, thread);

	// Streamed, so the dashboard shows while the directory finds them; null when it's offline.
	const projects = unlessOffline(locals.log, () => locals.app.projects.managedBy(viewer.did));

	// Which sections the account's profile has on, which decide the sidebar; null without a profile, or while the
	// directory is offline, when the sidebar shows everything.
	const sections = unlessOffline(locals.log, async () => {
		const profile = await locals.app.profiles.get(viewer.did);

		return profile && sectionsOf(profile.record);
	}).then((found) => found ?? null);

	const [managed, shown, needs] = await Promise.all([
		soon(projects),
		soon(sections),
		soon(needsYou(locals.app, viewer))
	]);

	return {
		projects: managed.value,
		sections: shown.value,
		unread: await locals.app.notifications.unreadCount(viewer.did),
		// Here rather than on Home because the sidebar counts them on every dashboard page. Reading a notification never
		// clears one: they go when the thing itself is answered.
		waiting: await needsCount(locals.app, viewer.did),
		needs: needs.value,
		access: projects.then((list) => accessWaiting(locals, list ?? []))
	};
};
