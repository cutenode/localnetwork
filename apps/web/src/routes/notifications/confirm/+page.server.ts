import { text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/**
 * The link in the email that confirms an address. Opening it only asks, so a mail app or scanner fetching the link
 * can't turn email on; the button posts from this page.
 */
export const load: PageServerLoad = async ({ locals, url }) => {
	const token = url.searchParams.get('t') ?? '';

	return { token, state: await locals.app.notificationChannels.confirmation(token) };
};

export const actions: Actions = {
	default: async ({ locals, request }) => {
		const token = text(await request.formData(), 't', 100);

		return { result: await locals.app.notificationChannels.confirmEmail(token) };
	}
};
