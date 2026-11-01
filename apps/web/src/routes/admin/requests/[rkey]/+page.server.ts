import { error } from '@sveltejs/kit';
import { diff } from '#lib/server/services/diff.ts';
import { DECISION_OUTCOMES } from '#lib/server/services/requests.ts';
import { act, requireStaff, text } from '#lib/server/http.ts';
import type { RequestView } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	requireStaff(locals, url);
	const { app } = locals;
	const request = await app.requests.byRkey(params.rkey);

	if (!request) error(404, 'Request not found');
	const project = request.projectUri ? await app.projects.byUri(request.projectUri) : null;
	const currentDetails = project ? await app.projects.details(project.uri) : null;

	const changes =
		request.kind === 'create'
			? diff({}, { profile: request.record.profile })
			: request.kind === 'update' && project
				? diff({ profile: project.profile }, { profile: request.record.profile ?? project.profile })
				: [];

	const detailChanges = request.record.details ? diff(currentDetails ?? {}, request.record.details, 'details') : [];

	const [history, trail] = await Promise.all([
		app.requests.list({ requester: request.requester.did, limit: 20 }),
		app.audit.list({ subject: request.uri, limit: 50 })
	]);

	// A verification vouches for the account itself, so only a claim from the act's own account (or a member's) leads
	// to one: a manager's or agent's account belongs to someone else.
	const relationship = request.record.claim?.relationship;

	const verifiable =
		locals.viewer?.isAdmin &&
		request.kind === 'claim' &&
		request.status === 'approved' &&
		(relationship === 'self' || relationship === 'member');

	const stale =
		request.kind === 'update' &&
		project &&
		request.record.baseRevision !== undefined &&
		request.record.baseRevision !== project.revision;

	return {
		request,
		project,
		listing: request.kind === 'list' ? await listingReview(locals.app, request) : null,
		changes,
		detailChanges,
		history: history.filter((item) => item.uri !== request.uri),
		trail,
		stale,
		// Streamed, so the page shows while the root's verifications and the requester's profile are read.
		verification: verifiable
			? app.verifications
					.statuses([request.requester.did])
					.then((statuses) => statuses?.[request.requester.did] ?? null)
			: null,
		hasAvatar: !!request.record.profile?.avatar,
		tracklists: (request.record.profile?.mixes ?? []).flatMap((mix, index) =>
			mix.tracklist ? [{ n: index + 1, title: mix.title }] : []
		)
	};
};

/**
 * What staff check before listing a DJ: the profile as it is now (and whether it changed since they asked), and any
 * solo project the same account manages, which would list them twice.
 */
async function listingReview(app: App.Locals['app'], request: RequestView) {
	const [profile, projects, listed] = await Promise.all([
		app.profiles.get(request.requester.did),
		app.projects.managedBy(request.requester.did),
		app.djListings.get(request.requester.did)
	]);

	return {
		profile,
		changed: !!profile && profile.cid !== request.record.listing?.cid,
		soloProjects: projects
			.filter((project) => project.status === 'active' && project.profile.kind === 'solo')
			.map((project) => ({ slug: project.slug, name: project.profile.name })),
		listed: !!listed
	};
}

export const actions: Actions = {
	decide: async ({ params, locals, url, request: httpRequest }) => {
		const viewer = requireStaff(locals, url);
		const request = await locals.app.requests.byRkey(params.rkey);

		if (!request) error(404, 'Request not found');
		const form = await httpRequest.formData();
		const choice = text(form, 'outcome', 32);
		const outcome = DECISION_OUTCOMES.find((value) => value === choice);

		return act(async () => {
			if (!outcome) return { error: 'Choose a decision' };
			await locals.app.requests.decide({
				actor: viewer,
				uri: request.uri,
				outcome,
				reason: text(form, 'reason', 10_000)
			});

			return {
				success:
					outcome === 'approved'
						? request.kind === 'list'
							? 'Approved and listed in Find DJs'
							: 'Approved and published'
						: outcome === 'rejected'
							? 'Rejected'
							: 'Sent back for changes'
			};
		});
	}
};
