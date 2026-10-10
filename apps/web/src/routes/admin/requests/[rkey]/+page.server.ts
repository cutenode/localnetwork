import { error } from '@sveltejs/kit';
import { diff } from '#lib/server/services/diff.ts';
import { DECISION_OUTCOMES } from '#lib/server/services/requests.ts';
import { act, requireStaff, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals }) => {
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

	const stale =
		request.kind === 'update' &&
		project &&
		request.record.baseRevision !== undefined &&
		request.record.baseRevision !== project.revision;

	return {
		request,
		project,
		changes,
		detailChanges,
		history: history.filter((item) => item.uri !== request.uri),
		trail,
		stale,
		hasAvatar: !!request.record.profile?.avatar,
		tracklists: (request.record.profile?.mixes ?? []).flatMap((mix, index) =>
			mix.tracklist ? [{ n: index + 1, title: mix.title }] : []
		)
	};
};

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
						? 'Approved and published'
						: outcome === 'rejected'
							? 'Rejected'
							: 'Sent back for changes'
			};
		});
	}
};
