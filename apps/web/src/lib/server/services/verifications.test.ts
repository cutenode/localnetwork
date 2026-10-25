import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BSKY_PROFILE, VERIFICATION } from '#lib/nsid.ts';
import { isCurrent } from '#lib/verification.ts';
import { recordCid } from '../atproto/repo-client';
import { ForbiddenError, UserError } from '../core';
import { HappyViewClient, HappyViewError } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { createTestServices, ROOT } from '../testing/core';
import { VerificationService } from './verifications';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

describe('VerificationService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	/** Sets the DJ's handle and Bluesky display name, as they'd have them on their PDS. */
	const identify = async (handle: string, displayName?: string) => {
		t.handles.set(DJ, handle);
		await t.net
			.client(DJ)
			.putRecord({ collection: BSKY_PROFILE, rkey: 'self', record: { $type: BSKY_PROFILE, displayName } });
	};

	const rootRecords = () => [...t.net.records.values()].filter((record) => record.uri.includes(`/${VERIFICATION}/`));

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD] });
		const admin = await t.viewer(ADMIN);
		await t.staff.grant(admin, MOD, 'moderator');

		const project = await t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		await t.projectAdmin.setManagers({ actor: admin, project, managers: [DJ] });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	it("writes a verification to the root's repo under the account's current handle and display name", async () => {
		await identify('neon.example', 'Neon Tide');
		expect(await t.verifications.grant(await t.viewer(ADMIN), DJ)).toEqual({
			handle: 'neon.example',
			displayName: 'Neon Tide'
		});

		const [record] = rootRecords();
		expect(record.uri).toMatch(new RegExp(`^at://${ROOT}/${VERIFICATION}/`));
		expect(record.value).toMatchObject({
			$type: VERIFICATION,
			subject: DJ,
			handle: 'neon.example',
			displayName: 'Neon Tide'
		});
		expect(await t.verifications.statuses([DJ, MOD])).toEqual({ [DJ]: 'verified', [MOD]: 'unverified' });

		const [entry] = await t.audit.list({ action: 'verification' });
		expect(entry).toMatchObject({
			action: 'verification.grant',
			subject: DJ,
			summary: 'Verified @neon.example on Bluesky'
		});
	});

	it('re-issues in place after a rename, since apps count one verification per issuer and account', async () => {
		const admin = await t.viewer(ADMIN);
		await identify('neon.example', 'Neon Tide');
		await t.verifications.grant(admin, DJ);
		const [first] = rootRecords();

		await identify('neon.example', 'NEON TIDE');
		expect(await t.verifications.statuses([DJ])).toEqual({ [DJ]: 'outdated' });
		await t.verifications.grant(admin, DJ);

		const records = rootRecords();
		expect(records).toHaveLength(1);
		expect(records[0].uri).toBe(first.uri);
		expect(records[0].value).toMatchObject({ displayName: 'NEON TIDE' });
		expect(await t.verifications.statuses([DJ])).toEqual({ [DJ]: 'verified' });
		await expect(t.verifications.grant(admin, DJ)).rejects.toThrow('@neon.example is already verified');
	});

	it('verifies only project managers, the only accounts whose display names HappyView keeps', async () => {
		t.handles.set(MOD, 'mod.example');
		await t.net
			.client(MOD)
			.putRecord({ collection: BSKY_PROFILE, rkey: 'self', record: { $type: BSKY_PROFILE, displayName: 'Mod' } });

		await expect(t.verifications.grant(await t.viewer(ADMIN), MOD)).rejects.toEqual(
			new UserError('Only project managers can be verified')
		);
		expect(rootRecords()).toEqual([]);
		expect(await t.verifications.live(MOD)).toEqual({ handle: 'mod.example', displayName: null });
	});

	it("refuses an account Bluesky wouldn't show a badge on", async () => {
		const admin = await t.viewer(ADMIN);
		await expect(t.verifications.grant(admin, DJ)).rejects.toThrow(/handle doesn't point back/);
		await identify('neon.example');
		await expect(t.verifications.grant(admin, DJ)).rejects.toThrow('@neon.example has no display name');
		await identify('neon.example', '');
		await expect(t.verifications.grant(admin, DJ)).rejects.toBeInstanceOf(UserError);
		expect(rootRecords()).toEqual([]);
	});

	it('revokes by deleting the record', async () => {
		const admin = await t.viewer(ADMIN);
		await identify('neon.example', 'Neon Tide');
		await t.verifications.grant(admin, DJ);
		await t.verifications.revoke(admin, DJ);

		expect(rootRecords()).toEqual([]);
		expect(await t.verifications.list()).toEqual([]);
		await expect(t.verifications.revoke(admin, DJ)).rejects.toThrow("That account isn't verified");
		expect((await t.audit.list({ action: 'verification' })).map((entry) => entry.action)).toEqual([
			'verification.revoke',
			'verification.grant'
		]);
	});

	it('lets only admins verify, and audits a moderator who tries', async () => {
		await identify('neon.example', 'Neon Tide');
		await expect(t.verifications.grant(await t.viewer(MOD), DJ)).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.verifications.revoke(await t.viewer(MOD), DJ)).rejects.toBeInstanceOf(ForbiddenError);

		expect(rootRecords()).toEqual([]);
		const denied = await t.audit.list({ action: 'verification', outcome: 'denied' });
		expect(denied.map((entry) => [entry.action, entry.actorDid])).toEqual([
			['verification.revoke', MOD],
			['verification.grant', MOD]
		]);
	});

	it('lists newest first and skips records that are not verifications', async () => {
		const root = t.net.client(ROOT);

		const older = {
			$type: VERIFICATION,
			subject: DJ,
			handle: 'a.example',
			displayName: 'A',
			createdAt: '2026-01-01T00:00:00.000Z'
		};

		const newer = { ...older, subject: MOD, handle: 'b.example', createdAt: '2026-02-01T00:00:00.000Z' };

		await root.createRecord({ collection: VERIFICATION, record: older });
		await root.createRecord({ collection: VERIFICATION, record: newer });
		await root.createRecord({ collection: VERIFICATION, record: { $type: VERIFICATION, subject: DJ } });

		expect((await t.verifications.list()).map((item) => item.handle)).toEqual(['b.example', 'a.example']);
		expect(await t.verifications.statuses([])).toEqual({});
	});

	it('asks to reload when the verification changed somewhere HappyView has not caught up with', async () => {
		const admin = await t.viewer(ADMIN);
		await identify('neon.example', 'Neon Tide');
		await t.verifications.grant(admin, DJ);
		const [record] = rootRecords();
		const key = `${ROOT}|${VERIFICATION}|${record.uri.split('/').at(-1)}`;

		// Another app re-issues it in the root's repo, and HappyView hasn't heard yet.
		const elsewhere = { ...record.value, displayName: 'Changed elsewhere' };
		t.net.records.set(key, { uri: record.uri, cid: await recordCid(elsewhere), value: elsewhere });
		await identify('neon.example', 'NEON TIDE');

		await expect(t.verifications.grant(admin, DJ)).rejects.toEqual(
			new UserError("@neon.example's verification changed since this page loaded. Reload and try again.", 409)
		);
		expect(t.net.records.get(key)?.value.displayName).toBe('Changed elsewhere');
		// HappyView re-read the root's verifications, so a reload shows the one that's there.
		expect((await t.verifications.list()).map((item) => item.displayName)).toEqual(['Changed elsewhere']);
	});

	it('reports no statuses and grants nothing while HappyView is down, rather than reading as unverified', async () => {
		await identify('neon.example', 'Neon Tide');

		const down = new VerificationService(
			t.core,
			new IndexedRepo(
				new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response('down', { status: 503 }) }),
				silentLogger,
				0
			),
			t.projects,
			{ resolve: async () => ({ handle: 'neon.example' }) }
		);

		expect(await down.statuses([DJ])).toBeNull();
		await expect(down.grant(await t.viewer(ADMIN), DJ)).rejects.toBeInstanceOf(HappyViewError);
		await expect(down.revoke(await t.viewer(ADMIN), DJ)).rejects.toBeInstanceOf(HappyViewError);
		expect(await down.live(DJ)).toEqual({ handle: 'neon.example', displayName: null });
		expect(rootRecords()).toEqual([]);
	});
});

describe('isCurrent', () => {
	const issued = { handle: 'neon.example', displayName: 'Neon Tide' };

	it('holds only while the handle and display name are exactly the ones verified', () => {
		expect(isCurrent(issued, { handle: 'neon.example', displayName: 'Neon Tide' })).toBe(true);
		expect(isCurrent(issued, { handle: 'neon.example', displayName: 'Neon Tide ' })).toBe(false);
		expect(isCurrent(issued, { handle: 'neon.other', displayName: 'Neon Tide' })).toBe(false);
		expect(isCurrent(issued, { handle: null, displayName: 'Neon Tide' })).toBe(false);
		expect(isCurrent({ handle: 'neon.example', displayName: '' }, { handle: 'neon.example', displayName: '' })).toBe(
			false
		);
	});
});
