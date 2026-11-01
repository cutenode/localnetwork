import * as fc from 'fast-check';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SPACE_TYPE } from '#lib/nsid.ts';
import { FakeRootClient } from '../testing/fakes';
import { createTestServices, ROOT } from '../testing/core';
import { type MemberAccess, reconcileMembers } from './space-members';

const DIDS = ['did:plc:ada', 'did:plc:bo', 'did:plc:cy', 'did:plc:di', 'did:plc:ed'];

const [ADA, BO, CY] = DIDS;

const readWrite = { read: true, write: true };

const readOnly = { read: true, write: false };

describe('reconcileMembers', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let spaces = 0;

	beforeEach(async () => {
		t = await createTestServices();
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	/** A new space under the root with `members` in it. */
	const space = async (members: Record<string, MemberAccess> = {}) => {
		const root = t.net.client(ROOT);
		const policy = { $type: 'com.atproto.simplespace.defs#memberListPolicy' } as const;

		const uri = await root.createSpace({
			spaceType: SPACE_TYPE.backstage,
			skey: `s${(spaces += 1)}`,
			readPolicy: policy,
			writePolicy: policy,
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});

		for (const [did, access] of Object.entries(members)) await root.putMember({ space: uri, did, ...access });

		return uri;
	};

	const access = (uri: string) => Object.fromEntries(t.net.spaces.get(uri)!.members);

	const sync = (
		uri: string,
		wanted: Record<string, MemberAccess>,
		options: { others?: string[]; joinable?: boolean } = {}
	) =>
		reconcileMembers(t.core, {
			uri,
			wanted: new Map(Object.entries(wanted)),
			others: options.others ?? [],
			joinable: options.joinable ?? true
		});

	it("adds who's missing, removes who isn't wanted, and corrects drifted access", async () => {
		const uri = await space({ [ADA]: readOnly, [BO]: readWrite });

		await sync(uri, { [ADA]: readWrite, [CY]: readOnly });

		expect(access(uri)).toEqual({ [ADA]: readWrite, [CY]: readOnly });
	});

	it('never adds or removes the others, even when they are wanted with other access', async () => {
		const uri = await space({ [ADA]: readWrite, [BO]: readOnly });

		await sync(uri, { [ADA]: readOnly, [CY]: readWrite }, { others: [ADA, BO] });

		expect(access(uri)).toEqual({ [ADA]: readWrite, [BO]: readOnly, [CY]: readWrite });
	});

	it("only keeps members in a space that isn't joinable", async () => {
		const uri = await space({ [ADA]: readOnly, [BO]: readOnly });

		await sync(uri, { [ADA]: readWrite, [CY]: readWrite }, { joinable: false });

		expect(access(uri)).toEqual({ [ADA]: readWrite });
	});

	it("records who's in, leaving out who it couldn't add, for a space with a ledger", async () => {
		const uri = await space({ [ADA]: readWrite, [BO]: readWrite });
		let ledger = [ADA, BO];
		t.net.failWrites(ROOT, 1);

		await reconcileMembers(t.core, {
			uri,
			wanted: new Map([
				[CY, readWrite],
				[ADA, readWrite]
			]),
			others: [],
			joinable: true,
			ledger: {
				recorded: async () => ledger,
				record: async (members) => {
					ledger = members;
				}
			}
		});

		expect(ledger).toEqual([ADA]);
		expect(access(uri)).toEqual({ [ADA]: readWrite });
	});

	it("fails closed on a space whose members it can't read, dropping who's no longer wanted from the ledger", async () => {
		const uri = await space({ [ADA]: readWrite, [BO]: readWrite });
		let ledger = [ADA, BO];
		vi.spyOn(FakeRootClient.prototype, 'listMembers').mockRejectedValueOnce(new Error('space host is down'));

		const run = (wanted: string[]) =>
			reconcileMembers(t.core, {
				uri,
				wanted: new Map(wanted.map((did) => [did, readWrite])),
				others: [],
				joinable: true,
				ledger: {
					recorded: async () => ledger,
					record: async (members) => {
						ledger = members;
					}
				}
			});

		await run([BO, CY]);
		expect(ledger).toEqual([BO]);
		expect(access(uri)).toEqual({ [ADA]: readWrite, [BO]: readWrite });

		await run([BO, CY]);
		expect(ledger).toEqual([BO, CY]);
		expect(access(uri)).toEqual({ [BO]: readWrite, [CY]: readWrite });
	});

	it('ends with the wanted members and the others as they were, and a second run writes nothing', async () => {
		const accessArb = fc.record({ read: fc.boolean(), write: fc.boolean() });
		const membersArb = fc.dictionary(fc.constantFrom(...DIDS), accessArb);

		await fc.assert(
			fc.asyncProperty(
				membersArb,
				membersArb,
				fc.subarray(DIDS),
				fc.boolean(),
				async (before, wanted, others, joinable) => {
					const uri = await space(before);

					await sync(uri, wanted, { others, joinable });

					const kept = Object.entries(wanted).filter(([did]) => !others.includes(did) && (joinable || did in before));

					const untouched = Object.entries(before).filter(([did]) => others.includes(did));
					expect(access(uri)).toEqual(Object.fromEntries([...kept, ...untouched]));

					t.net.failWrites(ROOT, 1);
					await sync(uri, wanted, { others, joinable });
					expect(t.net.failures.get(ROOT)?.count).toBe(1);
					t.net.failures.clear();
				}
			)
		);
	});
});
