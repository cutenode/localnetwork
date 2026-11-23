import { error, redirect } from '@sveltejs/kit';
import { type Capability, capabilitiesIn } from '#lib/capabilities.ts';
import type { ListedEvent } from '#lib/events.ts';
import { listedEvent } from '#lib/server/events-list.ts';
import { pageContext } from '#lib/server/edit-mode.ts';
import { cacheBriefly, requireFlag } from '#lib/server/http.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { spacePagePath } from '#lib/space-paths.ts';
import type { SpacePerson, Viewer } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

/**
 * The venue's dashboard pages the viewer can use: its events waiting for it with venue.approve, its settings with
 * page.edit or venue.block (staff can use both), and otherwise claiming it.
 */
function dashboardLinks(venue: VenueView, viewer: Viewer, person: SpacePerson | undefined, held: Capability[]) {
	const base = `/dashboard/venues/${venue.skey}`;
	const can = (capability: Capability) => viewer.isStaff || held.includes(capability);

	return [
		...(can('venue.approve') ? [{ label: 'Events to confirm', href: `${base}/events` }] : []),
		...(can('page.edit') || can('venue.block') ? [{ label: 'Settings', href: `${base}/settings` }] : []),
		...(person || viewer.isStaff ? [] : [{ label: 'Claim this venue', href: `${base}/claim` }])
	];
}

/**
 * `/venue/<key>/<name>`: a venue's page, behind `spacePages`. The key finds it and the name is only there to read (ADR
 * 0016), so any other name, or none, is sent on to the current one. It shows where the venue is and the events listed
 * there, but not its owners, who can't choose to show yet (LOC-510).
 */
export const load: PageServerLoad = async (event) => {
	const { locals, params, url } = event;

	requireFlag(locals, 'spacePages');
	const { app, viewer, flags } = locals;
	const venue = await app.venues.bySkey(params.key);

	if (!venue) error(404, 'Not found');
	const path = spacePagePath('venue', venue.skey, venue.name);

	if (url.pathname !== path) redirect(308, `${path}${url.search}`);
	const listed = await Promise.all((await app.events.listed(venue.space)).map((shown) => listedEvent(app, shown)));
	const { name, address } = venue;
	const place = [address.locality, address.country].filter(Boolean).join(', ');

	const person = viewer
		? (await app.sharedSpaces.people(venue.space, viewer)).find((someone) => someone.did === viewer.did)
		: undefined;

	const held = capabilitiesIn('venue', person?.capabilities ?? []);
	const { can } = pageContext(locals, 'venue', venue.space, held);

	// Its people and staff change what it shows from the dashboard, and come back to see it.
	cacheBriefly(event, 60, can ?? []);

	return {
		venue: { name, address, geo: venue.geo ?? null },
		events: listed.filter((shown): shown is ListedEvent => !!shown),
		dashboard: flags.peopleSpaces && viewer ? dashboardLinks(venue, viewer, person, held) : [],
		meta: pageMeta(app.config.publicUrl, {
			title: name,
			description: `${name} in ${place}: upcoming events on Local Network.`,
			path
		})
	};
};
