import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EpkView } from '#lib/epk.ts';
import { ForbiddenError, NotFoundError } from '../core';
import { ADMIN, CODJ, DJ, PROMOTER, pressKitServices, STRANGER } from '../testing/press-kits';
import { EpkViews, viewerCommitment } from './epk-views';

const MINUTE = 60_000;

const DAY = 1440 * MINUTE;

describe('EpkViews', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;
	let now: number;
	let views: EpkViews;
	let clubs: EpkView;

	beforeEach(async () => {
		f = await pressKitServices();
		now = Date.parse('2026-10-30T12:00:00Z');
		views = new EpkViews(f.t.core, f.t.epks, 90, () => now);
		clubs = await f.publish(DJ);
		await f.t.epks.invite(await f.t.viewer(DJ), f.kit(clubs), PROMOTER);
		await f.t.audit.idle();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	const view = async (did = PROMOTER, kit = clubs) => views.record(f.kit(kit), await f.t.viewer(did));

	const entries = (action: string) => f.t.audit.list({ action, limit: 500 });

	const salts = () => f.t.db.selectFrom('epkView').select(['id', 'salt', 'viewerDid']).execute();

	it("lists who viewed which press kit for the project's managers, newest first", async () => {
		const festivals = await f.publish(CODJ, { title: 'Festivals' });
		await f.t.epks.invite(await f.t.viewer(CODJ), f.kit(festivals), STRANGER);

		await view();
		now += MINUTE;
		await view(STRANGER, festivals);

		expect(
			(await views.list(f.project)).map((receipt) => [receipt.viewer.did, receipt.epk.title, receipt.epk.rkey])
		).toEqual([
			[STRANGER, 'Festivals', festivals.rkey],
			[PROMOTER, 'Clubs', clubs.rkey]
		]);
	});

	it('writes each view to the audit chain without naming the viewer, under an HMAC only the receipt can open', async () => {
		await view();
		await f.t.audit.idle();

		const [entry] = await entries('epk.view');
		const [{ id, salt }] = await salts();

		expect(entry).toMatchObject({ id, actorDid: f.t.core.rootDid, actorStatus: 'self', subject: clubs.uri });
		expect(JSON.parse(entry.details!)).toEqual({ viewer: viewerCommitment(salt, PROMOTER) });
		expect(JSON.stringify(entry)).not.toContain(PROMOTER);

		// Nor does the root's copy on the network.
		const copies = f.t.net.spaceRecordsIn(f.t.spaces.audit).filter((record) => record.uri.endsWith(`/${id}`));
		expect(copies).toHaveLength(1);
		expect(JSON.stringify(copies[0].value)).not.toContain(PROMOTER);
	});

	it('counts reloads within half an hour as one view', async () => {
		await view();
		now += 29 * MINUTE;
		await view();
		expect(await views.list(f.project)).toHaveLength(1);

		now += 2 * MINUTE;
		await view();
		expect(await views.list(f.project)).toHaveLength(2);
		expect(await entries('epk.view')).toHaveLength(2);
	});

	it("lets any of the project's managers delete a receipt, and the chain still verifies", async () => {
		await view();
		const [receipt] = await views.list(f.project);

		await views.remove(await f.t.viewer(CODJ), f.project, receipt.id);

		expect(await views.list(f.project)).toEqual([]);
		expect(await salts()).toEqual([]);
		expect(await entries('epk.view')).toHaveLength(1);
		expect(await entries('epk.forget')).toMatchObject([
			{ actorDid: CODJ, subject: clubs.uri, details: JSON.stringify({ entry: receipt.id }) }
		]);
		expect(await f.t.audit.verifyChain()).toMatchObject({ ok: true });
	});

	it("doesn't let anyone else delete a receipt, or a manager delete another project's", async () => {
		await view();
		const [receipt] = await views.list(f.project);

		await expect(views.remove(await f.t.viewer(PROMOTER), f.project, receipt.id)).rejects.toThrow(ForbiddenError);

		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [DJ]
		});

		await expect(views.remove(await f.t.viewer(DJ), other, receipt.id)).rejects.toThrow(NotFoundError);

		expect(await views.list(f.project)).toHaveLength(1);
	});

	it('expires receipts after the retention period, and the chain still verifies', async () => {
		await view();
		now += 10 * DAY;
		await view();

		now += 81 * DAY;
		expect((await views.list(f.project)).map((receipt) => receipt.viewedAt)).toEqual([
			new Date(now - 81 * DAY).toISOString()
		]);
		expect(await views.expire()).toBe(1);
		expect(await salts()).toHaveLength(1);
		expect(await views.expire()).toBe(0);
		expect(await entries('epk.view')).toHaveLength(2);
		expect(await f.t.audit.verifyChain()).toMatchObject({ ok: true });
	});

	it("never fails a press kit's page over its receipt", async () => {
		vi.spyOn(f.t.audit, 'record').mockRejectedValueOnce(new Error('disk full'));

		await expect(view()).resolves.toBeUndefined();
		expect(await views.list(f.project)).toEqual([]);
	});
});
