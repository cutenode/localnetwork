import { text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/**
 * The link in every email. Opening it only asks; the button posts from this page, since SvelteKit refuses a form post
 * from anywhere else, which rules out mail apps' one-click unsubscribe.
 */
export const load: PageServerLoad = async ({ locals, url }) => {
	const token = url.searchParams.get('t') ?? '';

	return { token, valid: token ? await locals.app.notificationChannels.hasToken(token) : false };
};

export const actions: Actions = {
	default: async ({ locals, request }) => {
		const token = text(await request.formData(), 't', 100);

		return { done: token ? await locals.app.notificationChannels.unsubscribe(token) : false };
	}
};
