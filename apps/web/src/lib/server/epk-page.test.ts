import { deflateSync } from 'node:zlib';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { loadEpk, ownedEpk, saveEpkForm } from './epk-page';
import { createTestServices } from './testing/core';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

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
		{
			type: 'image/png'
		}
	);
}

describe('press kit pages', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ] });
		const admin = await t.viewer(ADMIN);

		const created = await t.projectAdmin.create({
			actor: admin,
			profile: {
				name: 'Neon Tide',
				kind: 'solo',
				genres: ['techno'],
				location: { city: 'Berlin', country: 'DE' },
				mixes: [{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/neon/004' }]
			}
		});

		project = await t.projectAdmin.setManagers({ actor: admin, project: created, managers: [DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it('uploads new photos to the owner and files each where the form says', async () => {
		const save = async (visibility: string, file: File, opened?: { rkey: string; cid: string }) => {
			const form = new FormData();
			form.set('title', 'Clubs');
			form.set('cid', opened?.cid ?? '');
			form.set(`mixVisibility.${project.profile.mixes![0].url}`, 'private');
			form.append('newPhotos', file);
			form.set('newPhotoVisibility', visibility);
			form.set('newPhotoCredit', 'Mara Okafor');

			return saveEpkForm(t.epks, await t.viewer(DJ), project, form, [], opened?.rkey);
		};

		const epk = await save('public', png(3200, 1800, 'wide.png'));
		const [photo] = epk.record.photos!;

		expect(photo).toMatchObject({
			alt: 'Neon Tide, press photo',
			credit: 'Mara Okafor',
			aspectRatio: { width: 3200, height: 1800 }
		});
		expect(t.net.blobHolders.get(photo.image.ref.$link)?.has(DJ)).toBe(true);
		expect(epk.record.mixes).toBeUndefined();

		await save('private', png(1600, 1600, 'square.png'), epk);
		const content = t.net.spaceRecords.get(`${epk.space}|${DJ}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`)!.value;
		expect(content).toMatchObject({
			mixes: [{ title: 'Tidal Hours 004' }],
			photos: [{ aspectRatio: { width: 1600, height: 1600 } }]
		});

		expect((await loadEpk(t.epks, DJ, epk.rkey)).epk.record.title).toBe('Clubs');
		await expect(loadEpk(t.epks, DJ, 'missing')).rejects.toMatchObject({ status: 404 });
	});

	it("opens a press kit's dashboard pages to the manager who made it, under its own project", async () => {
		const CO = 'did:plc:co';
		const admin = await t.viewer(ADMIN);
		project = await t.projectAdmin.setManagers({ actor: admin, project, managers: [DJ, CO] });

		const other = await t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Halflight', kind: 'duo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } }
		});

		await t.projectAdmin.setManagers({ actor: admin, project: other, managers: [DJ] });
		const form = new FormData();
		form.set('title', 'Clubs');
		const epk = await saveEpkForm(t.epks, await t.viewer(DJ), project, form, []);
		const url = new URL('https://x.test/dashboard');

		const open = async (did: string, slug: string) => {
			const locals = { viewer: await t.viewer(did), app: { projects: t.projects, epks: t.epks } };

			return ownedEpk(locals, url, { slug, did: epk.did, rkey: epk.rkey }).then(
				(found) => found.epk.record.title,
				(cause: unknown) => cause
			);
		};

		expect(await open(DJ, project.slug)).toBe('Clubs');
		// Its other managers see it listed, but only the one who made it can change it for now.
		expect(await open(CO, project.slug)).toMatchObject({ status: 403 });
		expect(await open(DJ, other.slug)).toMatchObject({ status: 404 });
		expect(await open(ADMIN, project.slug)).toMatchObject({ status: 403 });
	});
});
