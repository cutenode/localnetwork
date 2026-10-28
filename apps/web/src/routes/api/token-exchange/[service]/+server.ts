import { exchangeToken } from '#lib/server/token-exchange/exchange.ts';
import type { RequestHandler } from './$types';

// locallibrary's plugins sign in to services that want a client secret through here: see exchange.ts.
export const POST: RequestHandler = ({ params, request, locals }) =>
	exchangeToken(params.service, request, { log: locals.log.child({ component: 'token-exchange' }) });
