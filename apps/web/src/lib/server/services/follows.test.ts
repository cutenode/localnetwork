import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { HappyViewClient } from '../happyview/client';
import { createTestServices } from '../testing/core';
import { FollowService } from './follows';

const ADMIN = 'did:plc:admin';

const FAN = 'did:plc:fan';

const MARA = 'did:plc:mara';

describe('FollowService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, FAN, MARA] });
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

	it('says whether an account follows anyone', async () => {
		expect(await t.follows.followsAny(FAN)).toBe(false);

		await t.follows.follow(await t.viewer(FAN), project);

		expect(await t.follows.followsAny(FAN)).toBe(true);
		expect(await t.follows.followsAny(ADMIN)).toBe(false);
	});

	/** Mara's profile, as the directory serves it. */
	const maraProfile = async () => {
		await t.profiles.save(await t.viewer(MARA), { basics: { name: 'Mara Lind' }, avatar: 'keep', swap: null });
		const profile = await t.profiles.get(MARA);

		if (!profile) throw new Error('no profile for Mara');

		return profile;
	};

	it('follows and unfollows a person, counted apart from the projects they follow', async () => {
		const mara = await maraProfile();
		const fan = await t.viewer(FAN);

		await t.follows.follow(fan, mara);
		await t.follows.follow(fan, mara);
		await t.follows.follow(await t.viewer(ADMIN), mara);

		expect(follows()).toHaveLength(1);
		expect(await t.follows.state(mara, FAN)).toEqual({ followers: 2, following: true });
		expect(await t.follows.state(project, FAN)).toEqual({ followers: 0, following: false });

		await t.follows.follow(fan, project);
		await t.follows.unfollow(fan, mara);

		expect(await t.follows.state(mara, FAN)).toEqual({ followers: 1, following: false });
		expect(await t.follows.state(project, FAN)).toEqual({ followers: 1, following: true });
		await t.audit.idle();
		const audited = await t.audit.list({ action: 'follow' });
		expect(
			audited.map(({ action, subject, subjectKind, summary }) => ({ action, subject, subjectKind, summary }))
		).toEqual(
			expect.arrayContaining([
				{ action: 'follow.create', subject: mara.uri, subjectKind: 'profile', summary: 'Followed Mara Lind' },
				{ action: 'follow.delete', subject: mara.uri, subjectKind: 'profile', summary: 'Unfollowed Mara Lind' },
				{ action: 'follow.create', subject: project.uri, subjectKind: 'project', summary: 'Followed Followed' }
			])
		);
	});

	it("doesn't let a person follow themselves", async () => {
		const mara = await maraProfile();

		await expect(t.follows.follow(await t.viewer(MARA), mara)).rejects.toThrow("You can't follow yourself");
		expect(await t.follows.state(mara, MARA)).toEqual({ followers: 0, following: false });
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
