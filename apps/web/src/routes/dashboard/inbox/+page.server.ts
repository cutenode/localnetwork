import { redirect } from '@sveltejs/kit';
import { requireViewer, safeNext, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

const PAGE_SIZE = 50;

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const before = url.searchParams.get('before') ?? undefined;
	// One more than a page, to know whether there's an older one.
	const items = await locals.app.notifications.list(viewer.did, { before, limit: PAGE_SIZE + 1 });
	const page = items.slice(0, PAGE_SIZE);

	const email = await locals.app.notificationChannels.view(viewer.did, 'email');

	return {
		items: page,
		older: items.length > PAGE_SIZE ? page.at(-1)?.id : undefined,
		emailPaused: email?.status === 'paused' ? email.pausedReason : null
	};
};

export const actions: Actions = {
	/** Marks an item read and goes where it leads. An item that's gone (pruned, or not theirs) leaves them here. */
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
