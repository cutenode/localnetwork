import { TID } from '@atproto/common-web';
import { ACCESS_PRESETS, type EpkAccess, type EpkFields, type EpkPhoto, type EpkView } from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { memberSpacePolicy } from '../atproto/repo-client';
import { toJson } from '../services/epk-fields';
import { createTestServices, ROOT } from './core';

export const ADMIN = 'did:plc:admin';

export const DJ = 'did:plc:dj';

export const CODJ = 'did:plc:codj';

export const PROMOTER = 'did:plc:promoter';

export const FRIEND = 'did:plc:friend';

export const STRANGER = 'did:plc:stranger';

/** A manager whose PDS can't write to spaces. */
export const NO_SPACES = 'did:plc:nospaces';

export const preset = (id: string): EpkAccess => ACCESS_PRESETS.find((entry) => entry.id === id)!.access;

/** A whole press kit form's values. */
export const values = (overrides: EpkFields = {}): EpkFields => ({
	title: 'Clubs',
	headline: 'Rolling techno for late rooms',
	bio: 'Short bio',
	access: preset('review'),
	mixes: [{ title: 'Public mix', url: 'https://soundcloud.com/dj/public' }],
	content: {
		bio: 'The long bio',
		mixes: [{ title: 'Promo', url: 'https://soundcloud.com/dj/promo' }],
		contact: { email: 'book@dj.test' },
		techRider: '2x CDJ-3000'
	},
	...overrides
});

/** Services with the project Neon Tide, managed by DJ and CODJ, and helpers for its press kits. */
export async function pressKitServices() {
	const t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ, CODJ, PROMOTER, FRIEND, STRANGER] });
	const admin = await t.viewer(ADMIN);

	const created = await t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
	});

	const fixture = {
		t,
		project: await t.projectAdmin.setManagers({ actor: admin, project: created, managers: [DJ, CODJ] }),

		async setManagers(managers: string[]): Promise<ProjectView> {
			fixture.project = await t.projectAdmin.setManagers({ actor: admin, project: fixture.project, managers });

			return fixture.project;
		},

		/** The press kit with the project as it is now. */
		kit: (epk: EpkView) => ({ epk, project: fixture.project }),

		/** The published version of the press kit `rkey` (none for a new one), as its editor opens on it. */
		async opened(rkey?: string): Promise<string | undefined> {
			return rkey && (await t.epks.get(fixture.project, rkey))?.epk.cid;
		},

		/** `did` saves a draft of the press kit `rkey` (a new one without it) from the whole form, and publishes it. */
		async publish(did: string, overrides: EpkFields = {}, rkey?: string): Promise<EpkView> {
			const actor = await t.viewer(did);
			const cid = await fixture.opened(rkey);
			const draft = await t.epks.drafts.save(actor, fixture.project, { rkey, cid, values: values(overrides) });
			const result = await t.epks.drafts.publish(actor, fixture.project, draft!.rkey);

			if (result.kind === 'conflict')
				throw new Error(`conflicts on ${result.conflicts.map((c) => c.field).join(', ')}`);

			return result.epk;
		},

		/** A photo uploaded to `did`'s repo. */
		async photo(did: string, name: string): Promise<EpkPhoto> {
			return { image: await t.net.client(did).uploadBlob(new TextEncoder().encode(name), 'image/png'), alt: name };
		},

		/** A press kit as `did` made one before the root held them: the record in their repo, the rest in their space. */
		async legacy(did: string, overrides: EpkFields = {}): Promise<EpkView> {
			const client = t.net.client(did);
			const rkey = TID.nextStr();
			const now = new Date().toISOString();
			const { content, ...visible } = values(overrides);

			const space = await client.createSpace({
				spaceType: SPACE_TYPE.epk,
				skey: rkey,
				policy: memberSpacePolicy({ publicWrite: true })
			});

			await client.putMember({ space, did: ROOT, read: true, write: true });
			await client.putSpaceRecord({
				space,
				collection: COLLECTION.epkContent,
				rkey: EPK_CONTENT_KEY,
				record: validateRecord(COLLECTION.epkContent, toJson({ ...content, updatedAt: now }))
			});
			await client.putRecord({
				collection: COLLECTION.epk,
				rkey,
				record: validateRecord(
					COLLECTION.epk,
					toJson({
						...visible,
						title: visible.title ?? 'Untitled',
						access: visible.access ?? preset('review'),
						project: fixture.project.uri,
						createdAt: now,
						updatedAt: now
					})
				)
			});
			t.epks.forget(did);

			return (await t.epks.get(fixture.project, rkey))!.epk;
		}
	};

	return fixture;
}
