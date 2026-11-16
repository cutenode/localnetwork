import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { parseSpaceRecordUri, spaceOfRecord } from '../../src/lib/server/atproto/spaces.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import type { HeldRecordView, Viewer } from '../../src/lib/types.ts';
import { load } from '../../src/routes/dashboard/account/data/+page.server.ts';
import { actions } from '../../src/routes/dashboard/account/data/[kind]/+page.server.ts';
import { TestNetwork } from './network.ts';

describe('deleting what the root wrote on your behalf, on a real atproto spaces network', () => {
	let net: TestNetwork;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'neon', 'lee', 'kai'], { admins: ['ada'] });

		// Lee and Kai's PDS can't write to spaces, so the root files what they send.
		for (const name of ['lee', 'kai']) {
			await net.app.accounts.recordLogin({
				did: net.user(name).did,
				handle: net.user(name).handle,
				pdsUrl: null,
				grantedScope: 'atproto',
				spacesCapable: false,
				publicWrites: false
			});
		}
	});
	afterAll(async () => {
		await net?.close();
	});

	const url = new URL('http://localhost/dashboard/account/data');

	const event = (viewer: Viewer, form?: Record<string, string>) => ({
		url,
		locals: { app: net.app, viewer, log: silentLogger },
		request: new Request(url, { method: 'POST', body: new URLSearchParams(form) })
	});

	/** The record as the root's repo has it now, or null once it's gone. */
	const inRootRepo = async (uri: string) => {
		const { collection, rkey } = parseSpaceRecordUri(uri)!;
		const root = await net.app.core.root();

		return root.getSpaceRecord({ space: spaceOfRecord(uri)!, repo: net.rootDid, collection, rkey });
	};

	it("lets someone delete the records the root wrote for them, and nobody else's", async () => {
		const [ada, lee, kai] = await Promise.all(['ada', 'lee', 'kai'].map((name) => net.viewer(name)));
		const profile = { kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } } as const;

		const created = await net.app.projectAdmin.create({ actor: ada, profile: { ...profile, name: 'Neon Tide' } });

		const project = await net.app.projectAdmin.setManagers({
			actor: ada,
			project: created,
			managers: [net.user('neon').did]
		});

		const leeRequest = await net.app.requests.submit({
			actor: lee,
			kind: 'create',
			profile: { ...profile, name: 'Proxy Party' }
		});

		const kaiRequest = await net.app.requests.submit({
			actor: kai,
			kind: 'create',
			profile: { ...profile, name: 'Kai Sounds' }
		});

		const space = await net.app.bookings.start({
			actor: lee,
			projectUri: project.uri,
			show: { title: 'Rooftop' },
			message: 'Free on the 5th?'
		});

		await net.app.bookings.message({ actor: lee, space, text: 'Any news?' });
		await net.settle();

		const listed: HeldRecordView[] = await (await load(event(lee))).records;
		expect(listed.map((record) => record.title).toSorted()).toEqual([
			'Inquiry in Rooftop',
			'Message in Rooftop',
			'New project: Proxy Party'
		]);
		const message = listed.find((record) => record.title === 'Message in Rooftop')!;
		expect(await inRootRepo(message.uri)).toMatchObject({ value: { text: 'Any news?', onBehalfOf: lee.did } });

		expect(await actions.delete(event(lee, { uri: message.uri }))).toEqual({ success: 'Deleted.' });
		expect(await inRootRepo(message.uri)).toBeNull();

		// Kai's request is the root's too, but it names Kai.
		expect(await actions.delete(event(lee, { uri: kaiRequest.uri }))).toMatchObject({
			status: 404,
			data: { error: "That isn't one of yours" }
		});
		expect(await actions.deleteAll(event(lee, { kind: 'request' }))).toEqual({ success: 'Deleted 1 record.' });
		expect(await inRootRepo(leeRequest.uri)).toBeNull();
		expect(await inRootRepo(kaiRequest.uri)).toMatchObject({ value: { onBehalfOf: kai.did } });

		// A sync of the spaces brings none of it back, and the thread's other side no longer sees the message.
		await net.settle();
		expect((await (await load(event(lee))).records).map((record) => record.title)).toEqual(['Inquiry in Rooftop']);
		expect(await net.app.requests.get(leeRequest.uri)).toBeNull();
		expect(await net.app.requests.get(kaiRequest.uri)).toMatchObject({ status: 'pending' });
		const { events } = await net.app.bookings.thread(await net.viewer('neon'), space);
		expect(events.map((shown) => shown.text)).toEqual(['Free on the 5th?']);

		const deleted = await net.app.audit.list({ actor: lee.did, action: 'record.delete' });
		expect(new Set(deleted.map((entry) => entry.subject))).toEqual(new Set([leeRequest.uri, message.uri]));
		expect(await net.app.audit.verifyChain()).toMatchObject({ ok: true });
		bookingSpace = space;
	});

	let bookingSpace = '';

	it('keeps a decided request without what it said, and the thread status as both sides see it', async () => {
		const [ada, lee] = await Promise.all(['ada', 'lee'].map((name) => net.viewer(name)));

		const rejected = await net.app.requests.submit({
			actor: lee,
			kind: 'create',
			profile: { name: 'Second Go', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
			message: 'Please'
		});

		await net.app.requests.decide({ actor: ada, uri: rejected.uri, outcome: 'rejected', reason: 'Not yet' });
		await net.app.bookings.setStatus({ actor: lee, space: bookingSpace, status: 'cancelled', note: 'Found someone' });
		await net.settle();

		expect(await actions.deleteAll(event(lee, { kind: 'request' }))).toEqual({ success: 'Deleted 1 record.' });
		expect(await actions.deleteAll(event(lee, { kind: 'booking' }))).toEqual({ success: 'Deleted 1 record.' });
		await net.settle();

		expect(await inRootRepo(rejected.uri)).toBeNull();
		const kept = await net.app.requests.get(rejected.uri);
		expect(kept).toMatchObject({ status: 'rejected', decision: { reason: 'Not yet' }, deletedAt: expect.any(String) });
		expect(kept?.record).not.toHaveProperty('message');
		expect(kept?.record).not.toHaveProperty('profile');
		const { thread, events } = await net.app.bookings.thread(await net.viewer('neon'), bookingSpace);
		expect(thread.status).toBe('cancelled');
		expect(events.map((shown) => shown.kind)).toEqual(['status']);
		expect(await (await load(event(lee))).records).toEqual([]);
	});
});
