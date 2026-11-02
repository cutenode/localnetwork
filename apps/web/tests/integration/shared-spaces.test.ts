import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION } from '../../src/lib/nsid.ts';
import { SpaceCredentialManager, XrpcResponseError } from '../../src/lib/server/atproto/credentials.ts';
import { AgentRootClient } from '../../src/lib/server/atproto/repo-client.ts';
import { parseSpaceUri } from '../../src/lib/server/atproto/spaces.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import {
	actions as spaceActions,
	load as spaceLoad
} from '../../src/routes/admin/spaces/[kind]/[skey]/+page.server.ts';
import { load as spacesLoad } from '../../src/routes/admin/spaces/+page.server.ts';
import { TestNetwork } from './network.ts';

const listRecordsOutput = z.object({
	records: z.array(z.object({ value: z.object({ subject: z.string() }) }))
});

describe('shared spaces on a real atproto spaces network', () => {
	let net: TestNetwork;
	let space: string;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'bobo', 'cyan', 'mira'], {
			admins: ['mira'],
			listingSources: { performer: { publicFields: async () => ({ name: 'Night Shift' }) } }
		});
		space = await net.app.sharedSpaces.create(await net.viewer('ada'), 'performer');
	});
	afterAll(async () => {
		await net?.close();
	});

	/** A space credential minted for `name`'s account, as any member's app would get one. */
	const credentialsOf = (name: string) =>
		new SpaceCredentialManager({
			source: async () => new AgentRootClient(net.user(name).did, net.user(name).agent),
			identity: net.app.identity,
			logger: silentLogger
		});

	/** The root's person records in the space, read with `credentials`. */
	const people = async (credentials: SpaceCredentialManager) => {
		const { records } = await credentials.query({
			space,
			service: net.pds.url,
			audience: net.rootDid,
			nsid: 'com.atproto.space.listRecords',
			params: { space, repo: net.rootDid, collection: COLLECTION.person },
			schema: listRecordsOutput
		});

		return records.map((record) => record.value.subject).sort();
	};

	const listing = async () =>
		(await (await net.app.core.root()).getRecord({ collection: COLLECTION.listing, rkey: parseSpaceUri(space)!.skey }))
			?.value;

	it('lets in an invitee who accepts, as a reader and writer, and nobody else', async () => {
		const bo = net.user('bobo');

		await net.app.spaceJoins.invite(await net.viewer('ada'), space, { did: bo.did, role: 'member' });
		expect((await (await net.app.core.root()).listMembers(space)).map((member) => member.did)).toEqual([
			net.user('ada').did
		]);

		await net.app.spaceJoins.accept(await net.viewer('bobo'), space, { hidden: false });
		const members = await (await net.app.core.root()).listMembers(space);
		expect(members.find((member) => member.did === bo.did)).toEqual({ did: bo.did, read: true, write: true });

		// Members can't list members, so the people list is records they read with their own credential.
		expect(await people(credentialsOf('bobo'))).toEqual([net.user('ada').did, bo.did].sort());
		await expect(people(credentialsOf('cyan'))).rejects.toBeInstanceOf(XrpcResponseError);
	});

	it('lists its members who have not hidden, once staff list it, and drops someone who hides', async () => {
		await net.app.sharedSpaces.setListed(await net.viewer('mira'), space, true);
		expect((await listing())?.people).toEqual([
			{ did: net.user('ada').did, role: 'member' },
			{ did: net.user('bobo').did, role: 'member' }
		]);

		await net.app.sharedSpaces.setHidden(await net.viewer('bobo'), space, net.user('bobo').did, true);
		expect((await listing())?.people).toEqual([{ did: net.user('ada').did, role: 'member' }]);
		expect(JSON.stringify(await listing())).not.toContain(net.user('bobo').did);
	});

	it('shows staff the space and its people in the console, and lets them take someone out', async () => {
		const staff = await net.viewer('mira');
		const { kind, skey } = { kind: 'performer', skey: parseSpaceUri(space)!.skey };
		const url = new URL(`http://127.0.0.1/admin/spaces/${kind}/${skey}`);
		const locals = { app: net.app, viewer: staff, log: silentLogger };

		const list = await spacesLoad({ url, locals, params: {} });
		expect(await list.spaces).toMatchObject([{ uri: space, kind, people: 2, listed: true }]);

		await expect(
			spacesLoad({ url, locals: { ...locals, viewer: await net.viewer('ada') }, params: {} })
		).rejects.toMatchObject({
			status: 403
		});

		const page = await spaceLoad({ url, locals, params: { kind, skey } });
		expect((await page.people)?.map((person) => [person.did, person.hidden])).toEqual([
			[net.user('ada').did, false],
			[net.user('bobo').did, true]
		]);

		await expect(
			spaceLoad({ url, locals: { ...locals, viewer: await net.viewer('ada') }, params: { kind, skey } })
		).rejects.toMatchObject({
			status: 403
		});
		await expect(spaceLoad({ url, locals, params: { kind: 'project', skey } })).rejects.toMatchObject({ status: 404 });

		const form = new FormData();
		form.set('did', net.user('bobo').did);
		await spaceActions.remove({
			url,
			locals,
			params: { kind, skey },
			request: new Request(url, { method: 'POST', body: form })
		});
		expect((await (await net.app.core.root()).listMembers(space)).map((member) => member.did)).toEqual([
			net.user('ada').did
		]);
	});

	it("keeps the removed member's credential working until it expires, and refuses them a new one", async () => {
		const ada = net.user('ada');
		const cy = net.user('cyan');
		const viewerAda = await net.viewer('ada');

		await net.app.spaceJoins.invite(viewerAda, space, { did: cy.did, role: 'member' });
		await net.app.spaceJoins.accept(await net.viewer('cyan'), space, { hidden: false });
		const credentials = credentialsOf('cyan');
		expect(await people(credentials)).toEqual([ada.did, cy.did].sort());

		await net.app.sharedSpaces.removePerson(viewerAda, space, cy.did);

		// removeMember doesn't revoke a credential already minted (nothing on the PDS sends notifyCredentialRevoked),
		// so it reads on for the rest of its 600-second life; a new one isn't issued.
		expect(await people(credentials)).toEqual([ada.did]);
		await expect(people(credentialsOf('cyan'))).rejects.toBeInstanceOf(XrpcResponseError);

		// Their PDS still takes a write into their own repo in the space, but the space doesn't: the authority
		// refuses its notification, the sync doesn't pick it up, and it's not the root's anyway.
		const now = new Date().toISOString();

		const forged = {
			subject: cy.did,
			role: 'member',
			capabilities: ['people.manage'],
			hidden: false,
			via: 'created',
			createdAt: now,
			updatedAt: now
		};

		await cy.client.createSpaceRecord({ space, collection: COLLECTION.person, rkey: cy.did, record: forged });
		await net.settle();
		expect((await net.app.sharedSpaces.people(space, await net.viewer('mira'))).map((person) => person.did)).toEqual([
			ada.did
		]);
	});
});
