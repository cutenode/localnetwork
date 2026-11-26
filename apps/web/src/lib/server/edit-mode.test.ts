import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { FieldError, ReauthError } from './core';
import { openEditor, pageContext, savePage } from './edit-mode';
import { silentLogger } from './logger';
import { performerRecord } from './services/performer-index';
import { createTestServices, ROOT } from './testing/core';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const URI = 'at:space/did:plc:root/party';

describe('pageContext', () => {
	const someone: Viewer = { did: BO, handle: null, roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const staff: Viewer = { ...someone, roles: ['moderator'], isStaff: true };

	it("gives one of the page's people what they can do on it, as its owner", () => {
		expect(pageContext({ viewer: someone }, 'party', URI, ['page.edit', 'events.manage'])).toEqual({
			subject: { kind: 'party', uri: URI },
			can: ['page.edit', 'events.manage'],
			role: 'owner'
		});
	});

	it('gives staff everything the page offers, and staff who are one of its people their own capabilities', () => {
		expect(pageContext({ viewer: staff }, 'act', URI, [])).toMatchObject({
			role: 'staff',
			can: ['page.edit', 'people.manage', 'listing.request', 'presskits.manage', 'bookings.answer', 'shows.manage']
		});
		expect(pageContext({ viewer: staff }, 'event', URI, [])).toMatchObject({
			role: 'staff',
			can: expect.arrayContaining(['events.manage'])
		});
		expect(pageContext({ viewer: staff }, 'venue', URI, ['page.edit'])).toEqual({
			subject: { kind: 'venue', uri: URI },
			can: ['page.edit'],
			role: 'owner-staff'
		});
	});

	it('says nothing to anyone who can do nothing there, signed out or not', () => {
		expect(pageContext({ viewer: someone }, 'party', URI, [])).toEqual({});
		expect(pageContext({ viewer: null }, 'party', URI, ['page.edit'])).toEqual({});
	});
});

describe('openEditor and savePage', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const edit = new URL('https://localnetwork.test/act/crew?edit');

	const opened = async () =>
		(await t.audit.list({ action: 'page.editAsStaff' })).map(
			({ actorDid, actorRole, subject, subjectKind, summary, details }) => ({
				actorDid,
				actorRole,
				subject,
				subjectKind,
				summary,
				details: JSON.parse(details ?? '{}')
			})
		);

	it('opens edit mode only when the URL asks and the viewer can edit, logging staff on a page that is not theirs', async () => {
		const locals = { viewer: await t.viewer(ADMIN), app: t };
		const asStaff = pageContext(locals, 'act', URI, []);

		expect(await openEditor(locals, new URL('https://localnetwork.test/act/crew'), asStaff, 'Crew')).toBe(false);
		expect(await openEditor(locals, edit, asStaff, 'Crew')).toBe(true);
		expect(await opened()).toEqual([
			{
				actorDid: ADMIN,
				actorRole: 'admin',
				subject: URI,
				subjectKind: 'space',
				summary: "Opened Crew's page to edit as staff",
				details: { page: 'act' }
			}
		]);

		// An event is a record in a party's space, so it's logged without a subject kind.
		expect(await openEditor(locals, edit, pageContext(locals, 'event', URI, []), 'Friday')).toBe(true);
		expect((await opened())[0]).toMatchObject({ subjectKind: null, details: { page: 'event' } });

		// Staff who are one of its people edit as one of them, which their saves log.
		expect(await openEditor(locals, edit, pageContext(locals, 'act', URI, ['page.edit']), 'Crew')).toBe(true);
		expect(await opened()).toHaveLength(2);

		const owner = { viewer: await t.viewer(ADA), app: t };
		expect(await openEditor(owner, edit, pageContext(owner, 'act', URI, ['bookings.answer']), 'Crew')).toBe(false);
		expect(await openEditor(owner, edit, pageContext(owner, 'act', URI, []), 'Crew')).toBe(false);
		expect(
			await openEditor(
				{ viewer: null, app: t },
				edit,
				{ subject: { kind: 'act', uri: URI }, can: ['page.edit'], role: 'owner' },
				'Crew'
			)
		).toBe(false);
	});

	describe('savePage', () => {
		const event = { url: new URL('https://localnetwork.test/act/crew?edit&/save'), locals: { log: silentLogger } };

		const form = (fields: Record<string, string>) => {
			const data = new FormData();

			for (const [name, value] of Object.entries(fields)) data.append(name, value);

			return data;
		};

		const page = (space: string) => t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.performer}|self`)!;

		const basics = (space: string) => {
			const subject = performerRecord(page(space).value)?.subject;

			return subject && 'basics' in subject ? subject.basics : undefined;
		};

		/** An act's page saved through the act service, the way a page's `?/save` action would. */
		const saveAct = (actor: Viewer, space: string, fields: Record<string, string>) =>
			savePage(
				event,
				form(fields),
				(version) =>
					t.sharedSpaces.updatePage(actor, space, {
						form: 'duo',
						basics: { name: fields.name },
						performer: {},
						avatar: 'keep',
						swap: version
					}),
				async () => ({ version: page(space).cid, values: { name: basics(space)?.name ?? '' } })
			);

		it('saves over the version the form opened on and goes back to the page without ?edit', async () => {
			const ada = await t.viewer(ADA);
			const space = await t.sharedSpaces.createAct(ada, { slug: 'crew', form: 'duo', basics: { name: 'Crew' } });

			await expect(saveAct(ada, space, { cid: page(space).cid, name: 'Crew Two' })).rejects.toMatchObject({
				status: 303,
				location: '/act/crew'
			});
			expect(basics(space)).toEqual({ name: 'Crew Two' });
		});

		it("hands back the latest values and version when someone saved since, and the form's own", async () => {
			const ada = await t.viewer(ADA);
			const space = await t.sharedSpaces.createAct(ada, { slug: 'crew', form: 'duo', basics: { name: 'Crew' } });
			const openedOn = page(space).cid;
			await expect(saveAct(await t.viewer(ADMIN), space, { cid: openedOn, name: 'Crew Two' })).rejects.toMatchObject({
				status: 303
			});

			expect(await saveAct(ada, space, { cid: openedOn, name: 'Crew Three' })).toMatchObject({
				status: 409,
				data: {
					error: 'The page changed since you opened it. Reload to see the latest, then save again.',
					values: { cid: openedOn, name: 'Crew Three' },
					reauth: false,
					conflict: { version: page(space).cid, values: { name: 'Crew Two' } }
				}
			});
			expect(basics(space)).toEqual({ name: 'Crew Two' });
		});

		it('names the refused field, asks to sign in again, and lets anything unexpected through', async () => {
			const latest = async () => ({ version: 'b', values: {} });

			expect(
				await savePage(
					event,
					form({ name: '' }),
					() => Promise.reject(new FieldError('Give it a name', 'name')),
					latest
				)
			).toMatchObject({
				status: 400,
				data: { error: 'Give it a name', field: { name: 'name', row: 0 }, values: { name: '' } }
			});
			expect(
				await savePage(event, form({}), () => Promise.reject(new ReauthError('Sign in again to save')), latest)
			).toMatchObject({
				status: 401,
				data: { reauth: true }
			});
			await expect(savePage(event, form({}), () => Promise.reject(new Error('boom')), latest)).rejects.toThrow('boom');
		});
	});
});
