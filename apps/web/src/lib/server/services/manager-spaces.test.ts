import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { spaceRecordUri } from '../atproto/spaces';
import { ForbiddenError } from '../core';
import { createTestServices, ROOT } from '../testing/core';
import { FakeRootClient } from '../testing/fakes';

const ADMIN = 'did:plc:admin';

const PROMOTER = 'did:plc:promoter';

const OLD = 'did:plc:old';

const NEW = 'did:plc:new';

describe('ManagerSpaces', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, PROMOTER, OLD, NEW] });

		project = await t.projectAdmin.create({
			actor: await t.viewer(ADMIN),
			profile: {
				name: 'Crew',
				kind: 'solo',
				genres: ['house'],
				location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
			},
			managers: [OLD]
		});
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	/** Opens a booking thread with the project, for a show on `date` if given, and moves it to `status`. */
	const thread = async (status: string, date?: string) => {
		const promoter = await t.viewer(PROMOTER);

		const space = await t.bookings.start({
			actor: promoter,
			projectUri: project.uri,
			show: { title: status, ...(date && { date }) },
			message: 'Free?'
		});

		if (status !== 'open') await t.bookings.setStatus({ actor: promoter, space, status });

		return space;
	};

	const members = (space: string) => [...(t.net.spaces.get(space)?.members.keys() ?? [])].sort();

	const setManagers = async (managers: string[]) => {
		project = await t.projectAdmin.setManagers({ actor: await t.viewer(ADMIN), project, managers });
	};

	const threadsOf = async (did: string) => (await t.bookings.threadsFor(did)).map((view) => view.space).sort();

	it("adds a new manager to the project's live booking threads, not its settled ones", async () => {
		const day = 24 * 60 * 60 * 1000;
		const open = await thread('open');
		const negotiating = await thread('negotiating');
		const upcoming = await thread('confirmed', new Date(Date.now() + day).toISOString());
		const undated = await thread('confirmed');
		const live = [open, negotiating, upcoming, undated];

		const settled = [
			await thread('confirmed', new Date(Date.now() - day).toISOString()),
			await thread('declined'),
			await thread('cancelled')
		];

		await setManagers([OLD, NEW]);

		for (const space of live) expect(members(space)).toEqual([NEW, OLD, PROMOTER].sort());

		for (const space of settled) expect(members(space)).toEqual([OLD, PROMOTER].sort());
		expect(await threadsOf(NEW)).toEqual(live.sort());
		const { thread: view } = await t.bookings.thread(await t.viewer(NEW), open);
		expect(view.participants.map((participant) => `${participant.role} ${participant.did}`).sort()).toEqual([
			`manager ${NEW}`,
			`manager ${OLD}`,
			`promoter ${PROMOTER}`
		]);
	});

	it('takes a removed manager out of every booking thread, in the space and in the app', async () => {
		await setManagers([OLD, NEW]);
		const open = await thread('open');
		const settled = await thread('declined');

		await setManagers([NEW]);

		for (const space of [open, settled]) expect(members(space)).toEqual([NEW, PROMOTER].sort());
		expect(await threadsOf(OLD)).toEqual([]);
		const old = await t.viewer(OLD);
		await expect(t.bookings.thread(old, open)).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.bookings.message({ actor: old, space: open, text: 'Still here?' })).rejects.toBeInstanceOf(
			ForbiddenError
		);

		// Their repo stays listed in the space, so a record synced from it now is ignored.
		await t.bookings.indexEvent({
			space: open,
			uri: spaceRecordUri(open, OLD, COLLECTION.bookingMessage, '3kaaaaaaaaaaa'),
			repoDid: OLD,
			collection: COLLECTION.bookingMessage,
			value: { text: 'Still here?', createdAt: new Date().toISOString() }
		});

		const { events } = await t.bookings.thread(await t.viewer(PROMOTER), open);
		expect(events.map((event) => event.text)).toEqual(['Free?']);
	});

	it("never adds or removes a booking's promoter, even when they manage the project", async () => {
		const open = await thread('open');

		await setManagers([OLD, PROMOTER]);
		expect(members(open)).toEqual([OLD, PROMOTER].sort());
		await setManagers([OLD]);
		expect(members(open)).toEqual([OLD, PROMOTER].sort());
		const { thread: view } = await t.bookings.thread(await t.viewer(PROMOTER), open);
		expect(view.participants.find((participant) => participant.did === PROMOTER)?.role).toBe('promoter');
	});

	it('changes nothing when the spaces already match the managers', async () => {
		const open = await thread('open');
		await setManagers([OLD, NEW]);
		// The audit log writes from the root too.
		await t.audit.idle();
		t.net.failWrites(ROOT, 1);

		await t.projects.reconcileManagerSpaces();
		await t.projects.reconcileManagerSpaces();

		// No member write was tried, so the failure is still waiting.
		expect(t.net.failures.get(ROOT)?.count).toBe(1);
		expect(members(open)).toEqual([NEW, OLD, PROMOTER].sort());
		expect(await threadsOf(NEW)).toEqual([open]);
	});

	it('finishes a sync that failed part way on the next reconcile', async () => {
		const open = await thread('open');
		await t.audit.idle();
		const ingest = t.net.onRecord;

		// The root's PDS fails both member writes right after it takes the new project record.
		t.net.onRecord = async (event) => {
			await ingest(event);
			t.net.onRecord = ingest;
			t.net.failWrites(ROOT, 2);
		};

		await setManagers([NEW]);

		expect(members(open)).toEqual([OLD, PROMOTER].sort());
		expect(await threadsOf(OLD)).toEqual([]);
		expect(await threadsOf(NEW)).toEqual([]);

		await t.projects.reconcileManagerSpaces();

		expect(members(open)).toEqual([NEW, PROMOTER].sort());
		expect(await threadsOf(NEW)).toEqual([open]);
	});

	it("leaves a space whose members it can't read for the next reconcile", async () => {
		const open = await thread('open');
		vi.spyOn(FakeRootClient.prototype, 'listMembers').mockRejectedValueOnce(new Error('space host is down'));

		await setManagers([OLD, NEW]);
		expect(members(open)).toEqual([OLD, PROMOTER].sort());

		await t.projects.reconcileManagerSpaces();
		expect(members(open)).toEqual([NEW, OLD, PROMOTER].sort());
	});
});
