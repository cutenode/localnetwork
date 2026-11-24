import { cookieName, type Locale } from '#lib/paraglide/runtime.js';
import { paraglideMiddleware } from '#lib/paraglide/server.js';

/** What `run` returns while the server answers a request whose cookie asks for `locale`, as the server hook runs pages. */
export async function inLocale<T>(locale: Locale, run: () => T): Promise<T> {
	const results: T[] = [];

	await paraglideMiddleware(
		new Request('https://localnetwork.test/', { headers: { cookie: `${cookieName}=${locale}` } }),
		() => {
			results.push(run());

			return new Response();
		}
	);

	return results[0];
}
