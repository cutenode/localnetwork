import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = ({ locals }) =>
	json(locals.app.oauth.metadata, { headers: { 'cache-control': 'public, max-age=600' } });
