import type { JsonValue } from '@atproto/lex-json';
import { json } from '@sveltejs/kit';
import { z } from 'zod';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { ServiceAuthError, verifyServiceJwt } from '#lib/server/atproto/service-auth.ts';
import type { RequestHandler } from './$types';

const notifyWriteInput = z.object({ space: z.string() });

/**
 * Write notifications forwarded by a space host for spaces we registered on.
 * Authenticated with service auth from the space authority; triggers a sync.
 */
export const POST: RequestHandler = async ({ request, locals }) => {
	const { app, log } = locals;
	let body: JsonValue;

	try {
		body = await request.json();
	} catch {
		return json({ error: 'InvalidRequest', message: 'Expected JSON' }, { status: 400 });
	}

	const input = notifyWriteInput.safeParse(body);
	const space = input.success ? input.data.space : '';
	const ref = parseSpaceUri(space);

	if (!ref) return json({ error: 'InvalidRequest', message: 'Bad space' }, { status: 400 });

	try {
		const payload = await verifyServiceJwt(request.headers.get('authorization'), {
			audience: app.config.syncService,
			lxm: 'com.atproto.space.notifyWrite',
			getSigningKey: (did, force) => app.identity.signingKey(did, force)
		});

		if (payload.iss !== ref.authority) throw new ServiceAuthError('Issuer is not the space authority');
	} catch (error) {
		log.warn({ err: error, space }, 'rejected notifyWrite');

		return json({ error: 'AuthenticationRequired', message: 'Invalid service auth' }, { status: 401 });
	}

	const watched = await app.db
		.selectFrom('spaceWatch')
		.select('spaceUri')
		.where('spaceUri', '=', space)
		.executeTakeFirst();

	if (!watched) return json({ error: 'SpaceNotFound' }, { status: 404 });
	void app.syncer.sync(space).catch((error) => log.warn({ err: error, space }, 'notified sync failed'));

	return json({});
};
