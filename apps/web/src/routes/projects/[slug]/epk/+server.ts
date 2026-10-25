import { redirect } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

/** The press kit list moved; links to it sit in emails, so the old address keeps working. */
export const GET: RequestHandler = ({ params, url }) => {
	redirect(308, `/projects/${params.slug}/press-kits${url.search}`);
};
