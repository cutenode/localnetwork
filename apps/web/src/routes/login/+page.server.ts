import { fail, redirect } from '@sveltejs/kit';
import { clientIp, safeNext, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	if (locals.viewer) redirect(303, safeNext(url.searchParams.get('next')));

	return {
		next: safeNext(url.searchParams.get('next')),
		error: url.searchParams.get('error'),
		reauth: locals.reauth === true
	};
};

export const actions: Actions = {
	default: async (event) => {
		const { locals, request } = event;
		const form = await request.formData();
		const handle = text(form, 'handle', 300).replace(/^@/, '');
		const next = safeNext(text(form, 'next', 500));

		if (!handle) return fail(400, { error: 'Enter your handle, like alice.bsky.social', handle });

		if (!locals.app.limits.login.take(`login:${clientIp(event)}`)) {
			return fail(429, { error: 'Too many sign-in attempts. Try again in a minute.', handle });
		}

		let url: URL;
		// An account that signed in before keeps the optional scopes it approved (OAuthService.authorize). Without it,
		// this is a first sign-in, or one whose handle doesn't resolve, which authorize reports below.
		const did = await locals.app.identity.resolveIdentifier(handle).catch(() => null);
		const account = did ? await locals.app.accounts.get(did) : undefined;

		try {
			url = await locals.app.oauth.authorize(handle, JSON.stringify({ next }), { account });
		} catch (error) {
			locals.log.info({ err: error, handle }, 'authorize failed');

			return fail(400, {
				error: "We couldn't find that account or reach its server. Check the handle and try again.",
				handle
			});
		}

		redirect(303, url, { external: true });
	}
};
