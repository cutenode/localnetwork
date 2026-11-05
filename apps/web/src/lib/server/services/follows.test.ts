import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { HappyViewClient } from '../happyview/client';
import { createTestServices } from '../testing/core';
import { FollowService } from './follows';

const ADMIN = 'did:plc:admin';

const FAN = 'did:plc:fan';

describe('FollowService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, FAN] });
		project = await t.projectAdmin.create({
			actor: await t.viewer(ADMIN),
			profile: { name: 'Followed', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	/** A FollowService over a directory that answers every query with `body`. */
	const answering = (body: string) =>
		new FollowService(
			t.core,
			new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response(body) }),
			0
		);

	const follows = () => [...t.net.records.keys()].filter((key) => key.startsWith(`${FAN}|${COLLECTION.follow}|`));

	it('passes on a directory answer that is not JSON, rather than calling following unavailable', async () => {
		await t.follows.follow(await t.viewer(FAN), project);

		await expect(answering('<html>').unfollow(await t.viewer(FAN), project)).rejects.toThrow(SyntaxError);
		expect(follows()).toHaveLength(1);
	});

	it('deletes nothing for a follow the directory names without a record key', async () => {
		await t.follows.follow(await t.viewer(FAN), project);
		const directory = answering(JSON.stringify({ followers: 1, follows: [`at://${FAN}/${COLLECTION.follow}`] }));

		await directory.unfollow(await t.viewer(FAN), project);

		expect(follows()).toHaveLength(1);
		await t.audit.idle();
		expect((await t.audit.list({ action: 'follow' })).map((entry) => entry.action)).toEqual(['follow.create']);
	});
});
