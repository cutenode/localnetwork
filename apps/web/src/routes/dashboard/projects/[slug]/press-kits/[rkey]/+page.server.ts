import { redirect } from '@sveltejs/kit';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import {
	describeConflicts,
	editorPhotos,
	formFields,
	loadEditor,
	managedKit,
	saveDraftForm
} from '#lib/server/epk-page.ts';
import { act } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

const NOTICES = {
	saved: 'Draft saved. The other managers can see it.',
	published: 'Published.',
	discarded: 'Draft discarded.',
	unchanged: 'Nothing changed from the published press kit.'
} as const;

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const editor = await loadEditor(locals, url, params);
	const { epk, draft, legacyMaker, project } = editor;
	const requests = epk && !legacyMaker ? await locals.app.epks.requests(epk) : [];
	const notice = Object.entries(NOTICES).find(([key]) => url.searchParams.get('notice') === key)?.[1] ?? null;

	return {
		epk,
		legacyMaker,
		title: epk?.record.title ?? draft?.changes.title ?? 'New press kit',
		draft: draft && { status: draft.status, updatedAt: draft.updatedAt },
		others: editor.others.map((other) => ({ author: other.author, updatedAt: other.updatedAt })),
		values: epkFormValues(project.profile.mixes ?? [], editor.fields),
		waiting: requests.filter((request) => request.status === 'pending').length,
		notice
	};
};

const back = (url: URL, notice: keyof typeof NOTICES) => `${url.pathname}?notice=${notice}`;

export const actions: Actions = {
	save: async ({ request, locals, url, params }) => {
		const editor = await loadEditor(locals, url, params);
		const form = await request.formData();

		const result = await act(
			() => saveDraftForm(locals.app.epks, editor.viewer, editor.project, form, editorPhotos(editor), params.rkey),
			submittedValues(form, editor.project.profile.mixes ?? [])
		);

		if (result === null) redirect(303, back(url, 'unchanged'));

		if ('uri' in result) redirect(303, back(url, 'saved'));

		return result;
	},
	publish: async ({ request, locals, url, params }) => {
		const editor = await loadEditor(locals, url, params);
		const form = await request.formData();
		const { epks } = locals.app;

		const result = await act(
			async () => {
				const draft = await saveDraftForm(epks, editor.viewer, editor.project, form, editorPhotos(editor), params.rkey);

				if (!draft) return { unchanged: true };
				const published = await epks.drafts.publish(editor.viewer, editor.project, draft.rkey);

				return published.kind === 'conflict'
					? { conflicts: describeConflicts(published.conflicts) }
					: { published: true };
			},
			submittedValues(form, editor.project.profile.mixes ?? [])
		);

		if ('unchanged' in result) redirect(303, back(url, 'unchanged'));

		if ('published' in result) redirect(303, back(url, 'published'));

		return result;
	},
	resolve: async ({ request, locals, url, params }) => {
		const editor = await loadEditor(locals, url, params);
		const form = await request.formData();
		const overwrite = formFields(form, 'overwrite');
		// The fields left unticked keep the published version.
		const keep = formFields(form, 'conflict').filter((field) => !overwrite.includes(field));

		const result = await act(async () => {
			const published = await locals.app.epks.drafts.publish(editor.viewer, editor.project, params.rkey, {
				overwrite,
				keep
			});

			return published.kind === 'conflict'
				? { conflicts: describeConflicts(published.conflicts) }
				: { published: true };
		});

		if ('published' in result) redirect(303, back(url, 'published'));

		return result;
	},
	discard: async ({ locals, url, params }) => {
		const editor = await loadEditor(locals, url, params);
		const result = await act(() => locals.app.epks.drafts.discard(editor.viewer, editor.project, params.rkey));

		if (result !== undefined) return result;

		redirect(303, editor.epk ? back(url, 'discarded') : `/dashboard/projects/${editor.project.slug}/press-kits`);
	},
	delete: async ({ locals, url, params }) => {
		const { viewer, kit } = await managedKit(locals, url, params);
		const result = await act(() => locals.app.epks.remove(viewer, kit));

		if (result === undefined) redirect(303, `/dashboard/projects/${kit.project.slug}/press-kits?deleted=1`);

		return result;
	}
};
