import { error } from '@sveltejs/kit';
import { ROLES, SPACE_KINDS } from '#lib/capabilities.ts';
import { sharedSpace } from '#lib/server/atproto/spaces.ts';
import { act, requireStaff, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** The space the route names, under the root. */
function spaceOf(locals: App.Locals, params: { kind: string; skey: string }): string {
	const kind = SPACE_KINDS.find((candidate) => candidate === params.kind);

	if (!kind) error(404, 'Not found');

	return sharedSpace(locals.app.rootDid, kind, params.skey);
}

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const viewer = requireStaff(locals, url);
	const { sharedSpaces, spaceJoins } = locals.app;
	const space = spaceOf(locals, params);
	const view = await sharedSpaces.get(space);

	if (!view) error(404, 'Not found');

	const failed = (what: string) => (cause: unknown) => {
		locals.log.warn({ err: cause, space }, `could not read a space's ${what}`);

		return null;
	};

	return {
		space: view,
		createdBy: await locals.app.accounts.view(view.createdBy),
		roles: Object.keys(ROLES[view.kind]),
		// Streamed, so the page shows at once and each part fills in.
		people: sharedSpaces.people(space, viewer).catch(failed('people')),
		pending: Promise.all([spaceJoins.pendingIn(space), spaceJoins.requestsFor(space)])
			.then(([{ invites, links }, requests]) => ({ invites, links, requests }))
			.catch(failed('invites and requests'))
	};
};

export const actions: Actions = {
	add: async ({ locals, url, params, request }) => {
		const viewer = requireStaff(locals, url);
		const form = await request.formData();

		return act(async () => {
			const did = await locals.app.identity.resolveIdentifier(text(form, 'subject', 300));
			const role = text(form, 'role', 32);
			await locals.app.sharedSpaces.addPerson(viewer, spaceOf(locals, params), {
				did,
				role,
				hidden: form.has('hidden')
			});

			return { success: 'Added' };
		});
	},
	role: async ({ locals, url, params, request }) => {
		const viewer = requireStaff(locals, url);
		const form = await request.formData();

		return act(async () => {
			const role = text(form, 'role', 32);
			await locals.app.sharedSpaces.updatePerson(viewer, spaceOf(locals, params), text(form, 'did', 300), { role });

			return { success: 'Role changed' };
		});
	},
	remove: async ({ locals, url, params, request }) => {
		const viewer = requireStaff(locals, url);
		const form = await request.formData();

		return act(async () => {
			await locals.app.sharedSpaces.removePerson(viewer, spaceOf(locals, params), text(form, 'did', 300));

			return { success: 'Removed' };
		});
	},
	list: async ({ locals, url, params }) => {
		const viewer = requireStaff(locals, url);

		return act(async () => {
			await locals.app.sharedSpaces.setListed(viewer, spaceOf(locals, params), true);

			return { success: 'Listed in the directory' };
		});
	},
	unlist: async ({ locals, url, params }) => {
		const viewer = requireStaff(locals, url);

		return act(async () => {
			await locals.app.sharedSpaces.setListed(viewer, spaceOf(locals, params), false);

			return { success: 'Taken out of the directory' };
		});
	}
};
