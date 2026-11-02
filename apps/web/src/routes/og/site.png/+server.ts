import SiteCard from '#lib/og/cards/SiteCard.svelte';
import { cardPaths } from '#lib/server/og/paths.ts';
import { renderCard } from '#lib/server/og/render.ts';
import { cards } from '#lib/server/og/serve.ts';
import type { RequestHandler } from './$types';

/** The link-preview card of the site's own pages. It changes only with a release, so it's kept for a day. */
export const GET: RequestHandler = ({ locals }) =>
	cards.respond(locals.log, cardPaths.site, () => renderCard(SiteCard, {}), { cacheControl: 'public, max-age=86400' });
