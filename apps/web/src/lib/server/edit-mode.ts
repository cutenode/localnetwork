import { fail, redirect } from '@sveltejs/kit';
import { CAPABILITIES, type Capability, capabilitiesIn, type SpaceKind } from '#lib/capabilities.ts';
import type { EditKind, EditRefusal, FieldValues, PageContext } from '#lib/components/edit/types.ts';
import { editUrl } from '#lib/components/edit/edit-url.ts';
import type { Viewer } from '#lib/types.ts';
import type { AuditLog } from './audit/log';
import { UserError } from './core';
import { act, stringValues, text } from './http';
import type { Logger } from './logger';
import { actorRole } from './services/requests';

/** The space each kind of page lives in, whose capabilities staff hold on it. */
const SPACE_OF: Record<EditKind, SpaceKind> = { act: 'performer', party: 'party', venue: 'venue', event: 'party' };

/**
 * What a page that shows one thing spreads into its load data for edit mode: what it's about, what the viewer `can`
 * do on it (their own capabilities, from the space's person record), and who they are to it. Staff who aren't one of
 * its people can do everything there, as staff. Anyone else gets nothing, so nobody else's page data says the edit
 * bar exists.
 */
export function pageContext(
	locals: { viewer: Viewer | null },
	kind: EditKind,
	uri: string,
	can: readonly Capability[]
): Partial<PageContext> {
	const subject = { kind, uri };

	if (can.length && locals.viewer)
		return { subject, can: [...can], role: locals.viewer.isStaff ? 'owner-staff' : 'owner' };

	if (!locals.viewer?.isStaff) return {};

	return { subject, can: capabilitiesIn(SPACE_OF[kind], Object.keys(CAPABILITIES)), role: 'staff' };
}

/**
 * Whether the page opens in edit mode: its URL asks (`?edit`) and the viewer can edit it. Staff opening someone else's
 * page are logged here, before the editor draws, so looking is on record even when nothing is saved after. A load
 * runs this for every way in (the Edit button, a link, a reload, a page without scripts), which is why the staff
 * Edit link isn't preloaded on hover.
 */
export async function openEditor(
	locals: { viewer: Viewer | null; app: { audit: AuditLog } },
	url: URL,
	context: Partial<PageContext>,
	name: string
): Promise<boolean> {
	const { subject, can, role } = context;

	if (!url.searchParams.has('edit') || !subject || !can?.includes('page.edit') || !locals.viewer) return false;

	if (role === 'staff')
		await locals.app.audit.record({
			actor: locals.viewer.did,
			actorRole: actorRole(locals.viewer),
			action: 'page.editAsStaff',
			subject: subject.uri,
			// An event is a record in its party's space, not a space of its own.
			subjectKind: subject.kind === 'event' ? undefined : 'space',
			summary: `Opened ${name}'s page to edit as staff`,
			details: { page: subject.kind }
		});

	return true;
}

/**
 * The body of a page's `?/save` action, the one way edit mode writes. `save` calls the page's service with the version
 * of the page the form was opened on (its hidden `cid`), so a change saved elsewhere since isn't overwritten. A saved
 * page goes back to its view. A refused one comes back as the form's values with why: the field at fault for a
 * `FieldError`, a sign-in for a `ReauthError`, and for a stale write (a `UserError` with status 409, as every service's
 * stale-write check throws) the page's `latest` values and version, which the editor sets beside what was typed.
 */
export async function savePage(
	event: { url: URL; locals: { log: Logger } },
	form: FormData,
	save: (version: string) => Promise<void>,
	latest: () => Promise<{ version: string; values: FieldValues }>
) {
	const values = stringValues(form);

	const result = await act(async () => {
		try {
			await save(text(form, 'cid', 200));
		} catch (cause) {
			if (!(cause instanceof UserError) || cause.status !== 409) throw cause;
			event.locals.log.info('a page save met a newer version', { path: event.url.pathname });

			return fail<EditRefusal>(409, { error: cause.message, values, reauth: false, conflict: await latest() });
		}

		return { saved: true };
	}, values);

	if (!('saved' in result)) return result;
	redirect(303, editUrl(event.url, false));
}
