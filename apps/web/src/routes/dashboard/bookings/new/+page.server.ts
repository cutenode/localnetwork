import { isValidDid } from '@atproto/syntax';
import { error, redirect } from '@sveltejs/kit';
import { performerFacts } from '#lib/components/PerformerFacts.svelte';
import { feeLevels, placeLabel, profileHref } from '#lib/format.ts';
import { rkeyOf } from '#lib/server/atproto/spaces.ts';
import { parseShow } from '#lib/server/forms/show.ts';
import { act, requireViewer, stringValues, text } from '#lib/server/http.ts';
import { takesInquiries } from '#lib/server/services/booking-performers.ts';
import { profileUri } from '#lib/server/services/profiles.ts';
import { inquiryDefaults, inquiryValues } from '#lib/shows.ts';
import type { Viewer } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

/** The viewer's planner show `rkey`, to start the inquiry from, or null when it can't be read. */
async function plannerShow(locals: App.Locals, viewer: Viewer, rkey: string) {
	if (!rkey || !viewer.spacesCapable) return null;

	try {
		return await locals.app.planner.show(viewer, rkey);
	} catch (cause) {
		// The form still opens, just without the show's details.
		locals.log.warn('planner show unavailable for inquiry', { err: cause, rkey });

		return null;
	}
}

/**
 * Who the inquiry goes to, as the form shows them: the listed DJ `?dj=<did>` names, else the active project
 * `?project=<slug>` names. Anyone else is a 404, and the DJ or a manager is sent back to the public page.
 */
async function recipient(locals: App.Locals, viewer: Viewer, url: URL) {
	const { app } = locals;
	const did = url.searchParams.get('dj');

	if (did !== null) {
		const [listing, profile, account] = isValidDid(did)
			? await Promise.all([app.djListings.get(did), app.profiles.get(did), app.accounts.view(did)])
			: [];

		const performer = takesInquiries(listing ?? null, profile ?? null) ? profile?.record.performer : undefined;

		if (!profile || !performer || !account) error(404, 'Pick a DJ to contact first');

		if (did === viewer.did) redirect(303, profileHref(account));
		const { name, location } = profile.record.basics;

		return {
			uri: profile.uri,
			name,
			avatarUrl: profile.avatarUrl,
			place: location ? placeLabel({ location }) : null,
			href: profileHref(account),
			dj: true,
			performer
		};
	}

	const project = await app.projects.bySlug(url.searchParams.get('project') ?? '');

	if (!project || project.status !== 'active') error(404, 'Pick a project to contact first');

	if (project.managers.includes(viewer.did)) redirect(303, `/projects/${project.slug}`);

	return {
		uri: project.uri,
		name: project.profile.name,
		avatarUrl: project.avatarUrl,
		place: placeLabel(project.profile),
		href: `/projects/${project.slug}`,
		dj: false,
		performer: project.profile
	};
}

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { performer, ...to } = await recipient(locals, viewer, url);

	// From the planner, the show's details go in; from the matcher, only its date, with the budget in the performer's
	// currency.
	const show = await plannerShow(locals, viewer, url.searchParams.get('show') ?? '');

	const values = show
		? inquiryValues(show.details)
		: {
				...inquiryDefaults,
				date: url.searchParams.get('date')?.slice(0, 10) ?? '',
				currency: feeLevels(performer)[0]?.currency ?? inquiryDefaults.currency
			};

	return {
		to,
		values,
		show: show && { rkey: show.rkey, title: show.details.title },
		// What the performer asks for, shown beside the budget so the offer can match it. Worded here, so the form's
		// page doesn't share PerformerFacts with the performer's public page (one more script for it to fetch).
		asks: performerFacts(performer)
			.filter((fact) => ['Fee', 'Fees', 'Slots', 'Set length'].includes(fact.term))
			.map((fact) => `${fact.term}: ${[fact.value].flat().join('; ')}.`),
		geocoder: locals.app.geocoder.enabled
	};
};

export const actions: Actions = {
	default: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const did = url.searchParams.get('dj');

		if (did !== null && !isValidDid(did)) error(404, 'Pick a DJ to contact first');

		// A DJ's profile URI goes as it is: starting the thread checks they're listed.
		const projectUri =
			did === null ? (await locals.app.projects.bySlug(url.searchParams.get('project') ?? ''))?.uri : profileUri(did);

		if (!projectUri) error(404, 'Project not found');
		const form = await request.formData();

		const result = await act(async () => {
			const space = await locals.app.bookings.start({
				actor: viewer,
				projectUri,
				show: parseShow(form),
				message: text(form, 'message', 20_000)
			});

			return { skey: rkeyOf(space) };
		}, stringValues(form));

		if ('skey' in result) redirect(303, `/dashboard/bookings/${result.skey}`);

		return result;
	}
};
