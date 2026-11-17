import { once } from 'node:events';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import type { JsonObject } from '@atproto/lex-json';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { memberSpacePolicy } from '../../src/lib/server/atproto/repo-client.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import { POST as confirmRegistration } from '../../src/routes/xrpc/com.atproto.space.confirmRegistration/+server.ts';
import { POST as notifySpaceUpdate } from '../../src/routes/xrpc/com.atproto.space.notifySpaceUpdate/+server.ts';
import { TestNetwork } from './network.ts';

const NOTE = 'computer.localnetwork.test.note';

const ROUTES = new Map([
	['com.atproto.space.confirmRegistration', confirmRegistration],
	['com.atproto.space.notifySpaceUpdate', notifySpaceUpdate]
]);

describe("space updates from a real atproto spaces host, sent to the app's own routes", () => {
	let net: TestNetwork | undefined;
	let server: Server;
	/** Every call the host made to the app, with what it sent and the status it got. */
	const calls: { nsid: string; body: JsonObject; status: number }[] = [];

	/** Answers the host as the app's routes would, once the network is up. Until then, as an endpoint it can't reach. */
	const answer = async (incoming: IncomingMessage, outgoing: ServerResponse) => {
		const chunks: Buffer[] = [];

		for await (const chunk of incoming) chunks.push(chunk);
		const body = Buffer.concat(chunks).toString();
		const nsid = (incoming.url ?? '').replace(/^\/xrpc\//, '');
		const route = ROUTES.get(nsid);
		const request = new Request(`http://localhost${incoming.url}`, { method: 'POST', body });

		const response =
			route && net
				? await route({ request, locals: { app: net.app, log: silentLogger } })
				: new Response(null, { status: 503 });

		calls.push({ nsid, body: JSON.parse(body), status: response.status });
		outgoing.writeHead(response.status, Object.fromEntries(response.headers));
		outgoing.end(Buffer.from(await response.arrayBuffer()));
	};

	beforeAll(async () => {
		server = createServer((incoming, outgoing) => void answer(incoming, outgoing));
		// A host name, not an IP: the PDS's fetch for outside endpoints refuses IP hosts.
		await once(server.listen(0, 'localhost'), 'listening');
		// SAFETY: a server listening on a TCP port reports an AddressInfo.
		const { port } = server.address() as AddressInfo;

		net = await TestNetwork.create(['ada', 'lee'], { admins: ['ada'], notifyEndpoint: `http://localhost:${port}` });
	});

	afterAll(async () => {
		await net?.close();
		await new Promise((resolve) => server.close(resolve));
	});

	const network = () => {
		if (!net) throw new Error('The network did not start');

		return net;
	};

	const watch = (space: string) =>
		network().app.db.selectFrom('spaceWatch').selectAll().where('spaceUri', '=', space).executeTakeFirst();

	const notes = (space: string) =>
		network()
			.app.db.selectFrom('spaceRecord')
			.select('rkey')
			.where('spaceUri', '=', space)
			.where('collection', '=', NOTE)
			.execute();

	it("still syncs, unregistered, while the endpoint can't confirm the registration", async () => {
		const { app } = network();

		expect(calls.filter((call) => call.nsid === 'com.atproto.space.confirmRegistration').length).toBeGreaterThan(0);
		expect(calls.every((call) => call.status === 503)).toBe(true);
		expect(await watch(app.spaces.staff)).toMatchObject({ registrationExpiresAt: null, lastError: null });
	});

	it('registers once the endpoint confirms, and syncs a write as soon as the host sends it', async () => {
		const { app, pds, rootDid } = network();
		const space = app.spaces.staff;

		await app.syncer.reauthorize(space);
		const { notifyToken, registrationExpiresAt } = (await watch(space))!;
		expect(registrationExpiresAt).not.toBeNull();
		expect(calls.at(-1)).toEqual({
			nsid: 'com.atproto.space.confirmRegistration',
			body: { space, token: notifyToken, challenge: expect.any(String) },
			status: 200
		});

		// Nothing polls here, so only the host's notice brings the write in.
		const root = await app.core.root();
		await root.createSpaceRecord({ space, collection: NOTE, rkey: 'heard', record: { $type: NOTE, text: 'hi' } });
		await pds.processAll();

		await expect.poll(() => notes(space), { timeout: 10_000 }).toEqual([{ rkey: 'heard' }]);
		expect(calls).toContainEqual({
			nsid: 'com.atproto.space.notifySpaceUpdate',
			body: {
				space,
				token: notifyToken,
				update: expect.objectContaining({ $type: 'com.atproto.space.defs#repoWrite', repo: rootDid })
			},
			status: 200
		});
	});

	it("drops a space on its host's deletion notice, once the authority refuses a fresh credential", async () => {
		const { app, pds } = network();
		const root = await app.core.root();

		const space = await root.createSpace({
			spaceType: 'computer.localnetwork.space.booking',
			skey: 'notified',
			policy: memberSpacePolicy()
		});

		await root.createSpaceRecord({ space, collection: NOTE, rkey: 'kept', record: { $type: NOTE, text: 'hi' } });
		await app.syncer.watch(space);
		await app.syncer.sync(space);
		expect(await notes(space)).toEqual([{ rkey: 'kept' }]);

		await root.deleteSpace(space);
		await pds.processAll();

		await expect.poll(() => watch(space), { timeout: 10_000 }).toBeUndefined();
		expect(await notes(space)).toEqual([]);
		expect(calls).toContainEqual({
			nsid: 'com.atproto.space.notifySpaceUpdate',
			body: expect.objectContaining({
				space,
				update: { $type: 'com.atproto.space.notifySpaceUpdate#spaceDeleted' }
			}),
			status: 200
		});
	});
});
