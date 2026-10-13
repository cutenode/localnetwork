import { deflateSync } from 'node:zlib';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { loadEpk, saveEpkForm } from './epk-page';
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
		const save = async (visibility: string, file: File, rkey?: string) => {
			const form = new FormData();
			form.set('title', 'Clubs');
			form.set(`mixVisibility.${project.profile.mixes![0].url}`, 'private');
			form.append('newPhotos', file);
			form.set('newPhotoVisibility', visibility);
			form.set('newPhotoCredit', 'Mara Okafor');

			return saveEpkForm(t.epks, await t.viewer(DJ), project, form, [], rkey);
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

		await save('private', png(1600, 1600, 'square.png'), epk.rkey);
		const content = t.net.spaceRecords.get(`${epk.space}|${DJ}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`)!.value;
		expect(content).toMatchObject({
			mixes: [{ title: 'Tidal Hours 004' }],
			photos: [{ aspectRatio: { width: 1600, height: 1600 } }]
		});

		expect((await loadEpk(t.epks, DJ, epk.rkey)).epk.record.title).toBe('Clubs');
		await expect(loadEpk(t.epks, DJ, 'missing')).rejects.toMatchObject({ status: 404 });
	});
});
