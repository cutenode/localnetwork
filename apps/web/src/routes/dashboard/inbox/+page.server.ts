import { redirect } from '@sveltejs/kit';
import { soon } from '#lib/server/home.ts';
import { requireViewer, safeNext, text } from '#lib/server/http.ts';
import type { NotificationChannel } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

const PAGE_SIZE = 50;

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const before = url.searchParams.get('before') ?? undefined;

	// One more than a page, to know whether there's an older one.
	const inbox = app.notifications.subjects(viewer.did, { before, limit: PAGE_SIZE + 1 }).then((rows) => {
		const page = rows.slice(0, PAGE_SIZE);

		return { rows: page, older: rows.length > PAGE_SIZE ? page.at(-1)?.id : undefined };
	});

	const [email, dm] = await Promise.all([
		app.notificationChannels.view(viewer.did, 'email'),
		app.notificationChannels.view(viewer.did, 'dm')
	]);

	// Only what the site can send and would send something on, so the line under the list never promises more.
	const sending = (channel: NotificationChannel, view: typeof email, available: boolean) =>
		available && view?.status === 'active' && view.topics.length > 0 ? [channel] : [];

	return {
		inbox: (await soon(inbox)).value,
		// Why each paused channel is, for a line pointing at the settings.
		paused: [email, dm].flatMap((channel) =>
			channel?.status === 'paused' && channel.pausedReason ? [channel.pausedReason] : []
		),
		alsoSent: [
			...sending('email', email, app.outbox.configured.includes('email')),
			...sending('dm', dm, Boolean(app.dm))
		]
	};
};

export const actions: Actions = {
	/**
	 * Marks a row's subject read and goes where it leads. An item that's gone (pruned, or not theirs) leaves them here.
	 */
	open: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const id = text(await request.formData(), 'id', 64);
		const href = id ? await locals.app.notifications.open(viewer.did, id) : null;

		redirect(303, safeNext(href, '/dashboard/inbox'));
	},

	readAll: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		await locals.app.notifications.markAllRead(viewer.did);
	}
};
