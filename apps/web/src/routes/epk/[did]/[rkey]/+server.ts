import { redirect } from '@sveltejs/kit';
import { movedPath } from '#lib/server/epk-page.ts';
import type { RequestHandler } from './$types';

/** A press kit's old address, named by the repo that held it, which links in emails still use. */
export const GET: RequestHandler = async ({ locals, params, url }) => {
	redirect(308, `${await movedPath(locals.app.epks, params.did, params.rkey)}${url.search}`);
};
