import { error, redirect } from '@sveltejs/kit';
import { UserError } from '#lib/server/core.ts';
import { parseShow } from '#lib/server/forms/show.ts';
import { act, all, requireViewer, stringValues } from '#lib/server/http.ts';
import type { Slot } from '#lib/server/services/planner.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const viewer = requireViewer(locals, url);
	const isNew = params.rkey === 'new';

	try {
		const show = isNew ? null : await locals.app.planner.show(viewer, params.rkey);

		if (!isNew && !show) error(404, 'Show not found');
		const saves = await locals.app.planner.saves(viewer);
		const uris = [...new Set([...saves.map((s) => s.project), ...(show?.lineup.map((l) => l.project) ?? [])])];
		const projects = Object.fromEntries(await locals.app.projects.byUris(uris));

		return { show, isNew, saves, projects };
	} catch (cause) {
		if (cause instanceof UserError) error(cause.status, cause.message);
		throw cause;
	}
};

export const actions: Actions = {
	default: async ({ params, locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		const statuses = form.getAll('slotStatus').map(String);
		const labels = form.getAll('slotLabel').map(String);

		const lineup: Slot[] = all(form, 'slotProject', 40).map((project, index) => {
			const status = statuses[index];
			const slot: Slot = { project, status: status === 'contacted' || status === 'confirmed' ? status : 'considering' };
			const label = labels[index]?.trim();

			if (label) slot.slot = label.slice(0, 64);

			return slot;
		});

		const result = await act(
			async () => {
				const rkey = await locals.app.planner.saveShow(viewer, {
					rkey: params.rkey === 'new' ? undefined : params.rkey,
					details: parseShow(form),
					lineup
				});

				return { rkey };
			},
			{ ...stringValues(form), lineup }
		);

		if ('rkey' in result) redirect(303, `/dashboard/planner/shows/${result.rkey}`);

		return result;
	}
};
