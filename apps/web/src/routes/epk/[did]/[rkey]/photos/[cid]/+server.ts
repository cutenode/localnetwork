import { redirect } from '@sveltejs/kit';
import { movedPath } from '#lib/server/epk-page.ts';
import type { RequestHandler } from './$types';

/** A photo's old address, from a press kit printed or emailed before it moved. */
export const GET: RequestHandler = async ({ locals, params, url }) => {
	redirect(308, `${await movedPath(locals.app.epks, params.did, params.rkey, `/photos/${params.cid}`)}${url.search}`);
};
