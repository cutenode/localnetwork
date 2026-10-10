import { json } from '@sveltejs/kit';
import { z } from 'zod';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { ServiceAuthError, verifyServiceJwt } from '#lib/server/atproto/service-auth.ts';
import type { RequestHandler } from './$types';

const notifySpaceDeletedInput = z.object({ space: z.string() });

export const POST: RequestHandler = async ({ request, locals }) => {
	const { app, log } = locals;
	const input = notifySpaceDeletedInput.safeParse(await request.json().catch(() => null));
	const space = input.success ? input.data.space : '';
	const ref = parseSpaceUri(space);

	if (!ref) return json({ error: 'InvalidRequest' }, { status: 400 });

	try {
		const payload = await verifyServiceJwt(request.headers.get('authorization'), {
			audience: app.config.syncService,
			lxm: 'com.atproto.space.notifySpaceDeleted',
			getSigningKey: (did, force) => app.identity.signingKey(did, force)
		});

		if (payload.iss !== ref.authority) throw new ServiceAuthError('Issuer is not the space authority');
	} catch (error) {
		log.warn({ err: error, space }, 'rejected notifySpaceDeleted');

		return json({ error: 'AuthenticationRequired' }, { status: 401 });
	}

	await app.syncer.onSpaceDeleted(space);

	return json({});
};
