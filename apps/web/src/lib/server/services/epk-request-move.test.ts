import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EpkView } from '#lib/epk.ts';
import { epkRequestSchema } from '#lib/epk-schema.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { validateRecord } from '../atproto/lexicons';
import { memberSpacePolicy } from '../atproto/repo-client';
import { epkRequestSpace } from '../atproto/spaces';
import { ROOT } from '../testing/core';
import { FakeRootClient } from '../testing/fakes';
import { DJ, FRIEND, NO_SPACES, pressKitServices, STRANGER } from '../testing/press-kits';
import { moveKitRequests } from './epk-request-move';

describe('moveKitRequests', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;
	let epk: EpkView;

	const requestSpace = () => epkRequestSpace(ROOT, f.project.rkey);

	const requestsIn = (space: string, repo?: string) =>
		f.t.net.spaceRecordsIn(space, repo).filter((record) => record.uri.includes(COLLECTION.epkRequest));

	/** A request written into the press kit's space by `writer` (the asker, or the root for them), as requests were. */
	const askedInKitSpace = async (did: string, writer = did): Promise<string> => {
		const record = validateRecord(COLLECTION.epkRequest, {
			epk: epk.uri,
			createdAt: new Date().toISOString(),
			...(writer === ROOT && { onBehalfOf: did })
		});

		const ref = await f.t.net
			.client(writer)
			.createSpaceRecord({ space: epk.space, collection: COLLECTION.epkRequest, record });

		await f.t.epks.indexRequest({ space: epk.space, uri: ref.uri, repoDid: writer, value: record });

		return ref.uri;
	};

	const move = () => moveKitRequests(f.t.core, f.t.epks);

	beforeEach(async () => {
		f = await pressKitServices();
		epk = await f.publish(DJ);
		// Press kit spaces took anyone's writes while requests went into them.
		await f.t.net.client(ROOT).updateSpace({ space: epk.space, policy: memberSpacePolicy({ publicWrite: true }) });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	it("moves requests out of a press kit's space with their answers, then stops it taking anyone's writes", async () => {
		const kit = f.kit(epk);
		const dj = await f.t.viewer(DJ);
		await askedInKitSpace(NO_SPACES, ROOT);
		const approved = await askedInKitSpace(STRANGER);
		const declined = await askedInKitSpace(FRIEND, ROOT);
		await f.t.epks.decide(dj, kit, approved, 'approve');
		await f.t.epks.decide(dj, kit, declined, 'decline');

		await move();

		// Nobody let into the press kit can read a request there any more.
		expect(requestsIn(epk.space)).toEqual([]);
		expect(
			requestsIn(requestSpace(), ROOT)
				.flatMap((record) => epkRequestSchema.parse(record.value).onBehalfOf ?? [])
				.sort()
		).toEqual([FRIEND, NO_SPACES, STRANGER].sort());

		const rows = await f.t.db
			.selectFrom('epkRequest')
			.select(['uri', 'spaceUri', 'requesterDid', 'status'])
			.orderBy('requesterDid')
			.execute();

		expect(rows.map(({ spaceUri, requesterDid, status }) => [spaceUri, requesterDid, status])).toEqual(
			[
				[requestSpace(), FRIEND, 'declined'],
				[requestSpace(), NO_SPACES, 'pending'],
				[requestSpace(), STRANGER, 'approved']
			].sort((a, b) => a[1].localeCompare(b[1]))
		);

		// The grant names the request where it is now.
		const copy = rows.find((row) => row.requesterDid === STRANGER)!.uri;
		const grant = f.t.net.spaceRecords.get(`${epk.space}|${ROOT}|${COLLECTION.epkGrant}|${STRANGER}`)?.value;
		expect(grant).toMatchObject({ subject: STRANGER, request: copy });
		expect(await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).toMatchObject({ kind: 'granted' });
		expect(await f.t.epks.accessState(kit, await f.t.viewer(FRIEND))).toEqual({ kind: 'declined' });

		expect(f.t.net.spaces.get(epk.space)?.policy).toEqual(memberSpacePolicy());
		expect(await f.t.audit.list({ action: 'system.space', subject: epk.space })).toHaveLength(1);

		// Once moved, a space is left alone.
		await move();
		expect(requestsIn(requestSpace())).toHaveLength(3);
		expect(await f.t.audit.list({ action: 'system.space', subject: epk.space })).toHaveLength(1);
	});

	it('waits for an asker to sign in before moving the request they wrote themselves', async () => {
		await askedInKitSpace(STRANGER);
		f.t.capable.delete(STRANGER);

		await move();
		expect(requestsIn(epk.space, STRANGER)).toHaveLength(1);
		expect(requestsIn(requestSpace())).toEqual([]);
		expect(f.t.net.spaces.get(epk.space)?.policy).toEqual(memberSpacePolicy({ publicWrite: true }));

		f.t.capable.add(STRANGER);
		await move();
		expect(requestsIn(epk.space)).toEqual([]);
		expect((await f.t.epks.requests(epk)).map((request) => request.uri)).toEqual(
			requestsIn(requestSpace(), ROOT).map((record) => record.uri)
		);
		expect(f.t.net.spaces.get(epk.space)?.policy).toEqual(memberSpacePolicy());
	});

	it('moves a request whole or not at all, so the managers never see it twice', async () => {
		await askedInKitSpace(STRANGER);
		vi.spyOn(FakeRootClient.prototype, 'deleteSpaceRecord').mockRejectedValueOnce(
			new XRPCError(502, 'UpstreamFailure')
		);

		await move();
		expect(requestsIn(epk.space, STRANGER)).toHaveLength(1);
		expect(requestsIn(requestSpace())).toEqual([]);
		expect(await f.t.epks.requests(epk)).toHaveLength(1);

		await move();
		expect(requestsIn(epk.space)).toEqual([]);
		expect((await f.t.epks.requests(epk)).map((request) => request.uri)).toEqual(
			requestsIn(requestSpace(), ROOT).map((record) => record.uri)
		);
	});

	it('leaves the requests of a press kit space whose press kit is gone, and carries on with the rest', async () => {
		await askedInKitSpace(NO_SPACES, ROOT);
		const gone = await f.publish(DJ, { title: 'Gone' });
		await f.t.net.client(ROOT).updateSpace({ space: gone.space, policy: memberSpacePolicy({ publicWrite: true }) });
		await f.t.net.client(ROOT).deleteRecord({ collection: COLLECTION.epk, rkey: gone.rkey });
		f.t.epks.forget();

		await move();
		expect(requestsIn(epk.space)).toEqual([]);
		expect(f.t.net.spaces.get(gone.space)?.policy).toEqual(memberSpacePolicy({ publicWrite: true }));
	});

	it("doesn't hold up startup on a space it can't read, and moves it on the next start", async () => {
		await askedInKitSpace(NO_SPACES, ROOT);
		vi.spyOn(FakeRootClient.prototype, 'getSpace').mockRejectedValueOnce(new XRPCError(502, 'UpstreamFailure'));

		await expect(move()).resolves.toBeUndefined();
		expect(requestsIn(epk.space)).toHaveLength(1);

		await move();
		expect(requestsIn(epk.space)).toEqual([]);
	});
});
