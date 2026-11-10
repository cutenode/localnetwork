import { requireGrant } from '#lib/server/forms/flags.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	await requireGrant(locals, url, 'flags');

	return { flags: await locals.app.flags.list() };
};
