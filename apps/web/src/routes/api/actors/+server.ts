import { json } from '@sveltejs/kit';
import type { ActorsResponse } from '#lib/actors.ts';
import { clientIp } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async (event) => {
	const { app } = event.locals;

	if (!app.limits.search.take(`actors:${clientIp(event)}`))
		return json({ actors: [] } satisfies ActorsResponse, { status: 429 });
	const q = event.url.searchParams.get('q') ?? '';

	return json({ actors: await app.actorSearch.search(q) } satisfies ActorsResponse, {
		headers: { 'cache-control': 'private, max-age=300' }
	});
};
