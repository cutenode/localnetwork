import { deflateSync } from 'node:zlib';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import {
	describeConflicts,
	editorPhotos,
	formFields,
	loadEditor,
	managedKit,
	movedPath,
	saveDraftForm
} from './epk-page';
import { ROOT } from './testing/core';
import { ADMIN, CODJ, DJ, pressKitServices, STRANGER, values } from './testing/press-kits';

function png(width: number, height: number, name: string): File {
	const ihdr = Buffer.alloc(25);
	ihdr.writeUInt32BE(13, 0);
	ihdr.write('IHDR', 4);
	ihdr.writeUInt32BE(width, 8);
	ihdr.writeUInt32BE(height, 12);
	ihdr[16] = 8;
	ihdr[17] = 2;

	return new File(
		[Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), ihdr, deflateSync(Buffer.from(name))])],
		name,
		{ type: 'image/png' }
	);
}

describe('press kit pages', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;
	const url = new URL('https://x.test/dashboard');

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	const locals = async (did: string) => ({
		viewer: await f.t.viewer(did),
		app: { projects: f.t.projects, epks: f.t.epks, accounts: f.t.accounts }
	});

	/** What a page helper resolves to, or the HTTP error it throws. */
	const outcome = <T>(promise: Promise<T>) =>
		promise.then(
			(value) => value,
			(cause: unknown) => cause
		);

	it('sends an old link, and its photos, to where the press kit lives now', async () => {
		const epk = await f.publish(DJ);
		const legacy = await f.legacy(CODJ, { title: 'Old' });
		const kit = `/projects/${f.project.slug}/press-kits`;

		expect(await movedPath(f.t.epks, ROOT, epk.rkey)).toBe(`${kit}/${epk.rkey}`);
		// Links made before the root held it named its maker's repo.
		expect(await movedPath(f.t.epks, DJ, epk.rkey, '/photos/bafy')).toBe(`${kit}/${epk.rkey}/photos/bafy`);
		expect(await movedPath(f.t.epks, CODJ, legacy.rkey)).toBe(`${kit}/${legacy.rkey}`);
		expect(await outcome(movedPath(f.t.epks, DJ, 'missing'))).toMatchObject({ status: 404 });
	});

	it("opens a press kit to every manager, moving a legacy one when its maker opens it, and tells the others who's to", async () => {
		const legacy = await f.legacy(DJ);

		const open = async (did: string, slug = f.project.slug) =>
			outcome(managedKit(await locals(did), url, { slug, rkey: legacy.rkey }));

		expect(await open(CODJ)).toMatchObject({ kit: { epk: { did: DJ, legacy: true } }, legacyMaker: { did: DJ } });
		expect(await open(STRANGER)).toMatchObject({ status: 403 });
		expect(await open(DJ)).toMatchObject({ kit: { epk: { did: ROOT, legacy: false } }, legacyMaker: null });
		expect(await open(CODJ)).toMatchObject({ kit: { epk: { did: ROOT } }, legacyMaker: null });

		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [DJ]
		});

		expect(await open(DJ, other.slug)).toMatchObject({ status: 404 });
	});

	it("opens the editor on the viewer's draft over the published press kit, and lists the others' drafts", async () => {
		const epk = await f.publish(DJ);

		const editor = async (did: string, rkey = epk.rkey) =>
			outcome(loadEditor(await locals(did), url, { slug: f.project.slug, rkey }));

		await f.t.epks.drafts.save(await f.t.viewer(CODJ), f.project, {
			rkey: epk.rkey,
			cid: epk.cid,
			values: values({ title: 'Clubs 2027' })
		});

		expect(await editor(DJ)).toMatchObject({
			draft: null,
			fields: { title: 'Clubs' },
			others: [{ author: { did: CODJ } }]
		});
		expect(await editor(CODJ)).toMatchObject({
			draft: { status: 'current' },
			fields: { title: 'Clubs 2027', content: { techRider: '2x CDJ-3000' } },
			others: []
		});

		// A new press kit's draft has nothing to show anyone but its author.
		const fresh = await f.t.epks.drafts.save(await f.t.viewer(CODJ), f.project, {
			values: values({ title: 'Festivals' })
		});

		expect(await editor(CODJ, fresh!.rkey)).toMatchObject({ epk: null, fields: { title: 'Festivals' } });
		expect(await editor(DJ, fresh!.rkey)).toMatchObject({ status: 404 });

		const legacy = await f.legacy(DJ, { title: 'Old' });
		expect(await editor(CODJ, legacy.rkey)).toMatchObject({ legacyMaker: { did: DJ }, published: {} });
	});

	it("saves the form as the viewer's draft, uploading new photos to their repo, without publishing", async () => {
		const epk = await f.publish(DJ);
		const form = new FormData();
		form.set('cid', epk.cid);
		form.set('title', 'Clubs 2027');
		form.append('newPhotos', png(3200, 1800, 'wide.png'));
		form.set('newPhotoVisibility', 'private');
		form.set('newPhotoCredit', 'Mara Okafor');

		const draft = await saveDraftForm(f.t.epks, await f.t.viewer(CODJ), f.project, form, [], epk.rkey);

		expect(draft).toMatchObject({ author: { did: CODJ }, fields: expect.arrayContaining(['title', 'content.photos']) });
		const [photo] = draft!.changes.content!.photos!;
		expect(photo).toMatchObject({
			alt: 'Neon Tide, press photo',
			credit: 'Mara Okafor',
			aspectRatio: { width: 3200, height: 1800 }
		});
		expect(f.t.net.blobHolders.get(photo.image.ref.$link)).toEqual(new Set([CODJ]));
		expect(f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${epk.rkey}`)?.value.title).toBe('Clubs');
		expect(f.t.net.spaceRecords.has(`${epk.space}|${ROOT}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`)).toBe(true);

		const editor = await loadEditor(await locals(CODJ), url, { slug: f.project.slug, rkey: epk.rkey });
		expect(editorPhotos(editor).map((entry) => entry.alt)).toEqual(['Neon Tide, press photo']);
	});

	it('writes conflicts out field by field, and reads back only the fields a draft can change', () => {
		const access = values().access!;

		expect(
			describeConflicts([
				{ field: 'headline', published: 'Theirs', previous: 'Old', draft: 'Mine' },
				{
					field: 'content.contact',
					published: { email: 'a@b.c', phone: '1' },
					previous: undefined,
					draft: { email: 'd@e.f' }
				},
				{ field: 'mixes', published: [{ title: 'One', url: 'https://x.test/1' }], previous: [], draft: undefined },
				{
					field: 'photos',
					published: [{ image: { $type: 'blob', ref: { $link: 'bafy' }, mimeType: 'image/png', size: 1 }, alt: 'A' }],
					previous: undefined,
					draft: undefined
				},
				{ field: 'access', published: access, previous: { ...access, anyone: 'none' }, draft: undefined }
			])
		).toEqual([
			{ field: 'headline', label: 'Headline', published: 'Theirs', previous: 'Old', draft: 'Mine' },
			{ field: 'content.contact', label: 'Contact', published: 'a@b.c · 1', previous: 'Empty', draft: 'd@e.f' },
			{ field: 'mixes', label: 'Mixes for everyone', published: 'One', previous: 'Empty', draft: 'Empty' },
			{ field: 'photos', label: 'Photos for everyone', published: '1 photo', previous: 'Empty', draft: 'Empty' },
			{
				field: 'access',
				label: 'Who can ask',
				published: 'You approve everyone',
				previous: 'Only promoters can ask',
				draft: 'Empty'
			}
		]);

		const form = new FormData();

		for (const value of ['title', 'content.bio', 'project', '__proto__']) form.append('overwrite', value);
		expect(formFields(form, 'overwrite')).toEqual(['title', 'content.bio']);
	});
});
