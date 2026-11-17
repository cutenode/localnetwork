import { json } from '@sveltejs/kit';
import { z } from 'zod';
import type { RequestHandler } from './$types';

const confirmRegistrationInput = z.object({
	space: z.string(),
	token: z.string().max(512),
	challenge: z.string().max(512)
});

/**
 * A space host checking, before it registers this app for a space's updates, that the app asked: the token has to be
 * the one the syncer holds for that space. The challenge goes back as it came.
 */
export const POST: RequestHandler = async ({ request, locals }) => {
	const { app, log } = locals;
	const input = confirmRegistrationInput.safeParse(await request.json().catch(() => null));

	if (!input.success) return json({ error: 'InvalidRequest' }, { status: 400 });
	const { space, token, challenge } = input.data;

	if (!(await app.syncer.isRegistered(space, token))) {
		log.warn('refused to confirm a notify registration', { space });

		return json({ error: 'AuthenticationRequired' }, { status: 401 });
	}

	return json({ challenge });
};
