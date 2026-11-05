import { fail, redirect } from '@sveltejs/kit';
import { clientIp, safeNext, text } from '#lib/server/http.ts';
import { accountHosts } from '#lib/server/sign-in.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	const next = safeNext(url.searchParams.get('next'));

	if (locals.viewer) redirect(303, next);

	return { next, hosts: accountHosts(locals.app.config) };
};

export const actions: Actions = {
	/** Starts sign-up at the chosen host, which sends the new account straight back through the OAuth callback. */
	default: async (event) => {
		const { locals, request } = event;
		const form = await request.formData();
		const next = safeNext(text(form, 'next', 500));
		const host = accountHosts(locals.app.config).find((item) => item.service === text(form, 'service', 200));

		if (!host) return fail(400, { error: 'Pick a host from the list.' });

		if (!locals.app.limits.login.take(`login:${clientIp(event)}`)) {
			return fail(429, { error: 'Too many sign-in attempts. Try again in a minute.' });
		}

		let url: URL;

		try {
			url = await locals.app.oauth.authorize(host.service, JSON.stringify({ next }), { prompt: 'create' });
		} catch (error) {
			locals.log.info('sign-up authorize failed', { err: error, service: host.service });

			return fail(502, { error: `We couldn't reach ${host.name}. Try again, or pick another host.` });
		}

		redirect(303, url, { external: true });
	}
};
