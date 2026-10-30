import { once } from 'node:events';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { IncludeScope, ScopePermissions, ScopePermissionsTransition, SpacePermission } from '@atproto/oauth-scopes';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CALENDAR_EVENT, CALENDAR_RSVP, COLLECTION, PERMISSIONS, SPACE_TYPE } from '#lib/nsid.ts';
import { loadConfig } from '../config';
import { openDatabase } from '../db';
import { silentLogger } from '../logger';
import { type Loopback, rebindToLoopback } from '../testing/rebind';
import { IdentityResolver } from './identity';
import { PERMISSION_SET } from './lexicons';
import {
	clientMetadata,
	DECLARED_SCOPES,
	grantsAll,
	grantsProfile,
	grantsPublicRecords,
	grantsSpaces,
	heldOptionalScopes,
	isLoopback,
	OAuthService,
	OptionalScopeUnsupported,
	SCOPES,
	type SignedInAccount
} from './oauth';

/** A fetch for services whose client never reaches the network. */
const offline: typeof globalThis.fetch = async (input) => {
	throw new Error(`no network in this test: ${new Request(input).url}`);
};

const config = (publicUrl: string) =>
	loadConfig({
		ROOT_IDENTIFIER: 'did:plc:root',
		ROOT_PASSWORD: 'x',
		PUBLIC_URL: publicUrl,
		HAPPYVIEW_URL: 'http://127.0.0.1:8000'
	});

describe('OAuth scopes', () => {
	const { full, base } = SCOPES;
	const user = 'did:plc:user';
	const root = 'did:plc:root';

	// What the PDS grants for `full`: the permission set expanded, with the planner's `self` authority resolved.
	const granted = [
		...new IncludeScope(PERMISSIONS)
			.toPermissions(PERMISSION_SET)
			.map((permission) =>
				permission instanceof SpacePermission ? permission.withResolvedAuthority(user) : permission
			)
			.map(String),
		...full.split(' ').filter((scope) => !scope.startsWith('include:'))
	];

	/**
	 * What accounts hold from before the profiles permissions: the grant `full` got from the permission set and scopes as
	 * they were, written out as the PDS stores it, so it doesn't change with them.
	 */
	const beforeProfiles = [
		'space:computer.localnetwork.space.audit?authority=*&skey=main&collection=computer.localnetwork.audit.entry&action=read_self&action=create',
		'space:computer.localnetwork.space.moderation?authority=*&skey=main&collection=computer.localnetwork.moderation.request&action=read_self&action=create&action=update&action=delete',
		'space:computer.localnetwork.space.booking?authority=*&collection=computer.localnetwork.booking.inquiry&collection=computer.localnetwork.booking.message&collection=computer.localnetwork.booking.status&action=read_self&action=create&action=update&action=delete',
		`space:computer.localnetwork.space.planner?authority=${user}&skey=self&collection=computer.localnetwork.planner.save&collection=computer.localnetwork.planner.show&manage=create&manage=update`,
		'repo:computer.localnetwork.epk',
		'repo:computer.localnetwork.show',
		'repo:computer.localnetwork.follow?action=create&action=delete',
		`space:computer.localnetwork.space.epk?authority=${user}&collection=computer.localnetwork.epk.content&collection=computer.localnetwork.epk.grant&manage=create&manage=update&manage=delete`,
		'space:computer.localnetwork.space.backstage?authority=*&collection=computer.localnetwork.epk.draft&action=read_self&action=create&action=update&action=delete',
		'space:computer.localnetwork.space.epk?authority=*&collection=computer.localnetwork.epk.request&action=read_self&action=create&action=delete',
		'atproto',
		'blob?accept=image/jpeg&accept=image/png&accept=image/webp&accept=application/json',
		'repo:community.lexicon.calendar.event?action=create&action=update&action=delete'
	];

	it('requests the permission set, keeping explicit only what a set cannot hold', () => {
		expect(full.split(' ').filter((scope) => IncludeScope.fromString(scope)?.nsid === PERMISSIONS)).toHaveLength(1);
		// A permission outside the set's own NSID authority would be dropped silently when the PDS expands it.
		expect(new IncludeScope(PERMISSIONS).toPermissions(PERMISSION_SET)).toHaveLength(PERMISSION_SET.permissions.length);
	});

	it("allows every space write the app makes with a user's session, and no reads of others' records", () => {
		const permissions = new ScopePermissions(granted);
		const audit = { type: SPACE_TYPE.audit, authority: root, skey: 'main' } as const;
		const moderation = { type: SPACE_TYPE.moderation, authority: root, skey: 'main' } as const;
		const booking = { type: SPACE_TYPE.booking, authority: root, skey: '3mxq3whd6gi2d' } as const;
		const planner = { type: SPACE_TYPE.planner, authority: user, skey: 'self' } as const;

		expect(permissions.allowsSpace({ ...audit, action: 'create', collection: COLLECTION.auditEntry })).toBe(true);

		for (const action of ['create', 'update', 'delete'] as const) {
			expect(permissions.allowsSpace({ ...moderation, action, collection: COLLECTION.request })).toBe(true);

			for (const collection of [COLLECTION.bookingInquiry, COLLECTION.bookingMessage, COLLECTION.bookingStatus])
				expect(permissions.allowsSpace({ ...booking, action, collection })).toBe(true);

			for (const collection of [COLLECTION.plannerSave, COLLECTION.plannerShow])
				expect(permissions.allowsSpace({ ...planner, action, collection })).toBe(true);
		}

		for (const manage of ['create', 'update'] as const)
			expect(permissions.allowsSpace({ ...planner, manage })).toBe(true);

		for (const space of [audit, moderation, booking])
			expect(permissions.allowsSpace({ ...space, action: 'read' })).toBe(false);
		expect(permissions.allowsSpace({ ...planner, authority: 'did:plc:someoneelse', action: 'read' })).toBe(false);
		expect(
			permissions.allowsSpace({
				type: SPACE_TYPE.staff,
				authority: root,
				skey: 'main',
				action: 'create',
				collection: COLLECTION.staffRole
			})
		).toBe(false);
	});

	it("lets a DJ run their own press kit spaces, and anyone ask for access to someone else's", () => {
		const permissions = new ScopePermissions(granted);
		const own = { type: SPACE_TYPE.epk, authority: user, skey: '3mabc' } as const;
		const theirs = { ...own, authority: 'did:plc:other' } as const;

		expect(permissions.allowsRepo({ collection: COLLECTION.epk, action: 'create' })).toBe(true);
		expect(permissions.allowsRepo({ collection: COLLECTION.project, action: 'create' })).toBe(false);
		expect(permissions.allowsSpace({ ...own, manage: 'update' })).toBe(true);
		expect(permissions.allowsSpace({ ...own, action: 'create', collection: COLLECTION.epkGrant })).toBe(true);
		expect(permissions.allowsSpace({ ...theirs, action: 'create', collection: COLLECTION.epkRequest })).toBe(true);
		expect(permissions.allowsSpace({ ...theirs, action: 'create', collection: COLLECTION.epkGrant })).toBe(false);
		expect(permissions.allowsSpace({ ...theirs, action: 'read' })).toBe(false);
	});

	it("lets users keep their profile, edit their sets, draft in a performer's space, and answer events", () => {
		const permissions = new ScopePermissions(granted);
		const performer = { type: SPACE_TYPE.performer, authority: root, skey: '3mperf' } as const;

		for (const action of ['create', 'update', 'delete'] as const) {
			expect(permissions.allowsRepo({ collection: COLLECTION.profile, action })).toBe(true);
			expect(permissions.allowsRepo({ collection: CALENDAR_RSVP, action })).toBe(true);
			expect(permissions.allowsSpace({ ...performer, action, collection: COLLECTION.epkDraft })).toBe(true);
			// Only staff list a DJ, from the root account.
			expect(permissions.allowsRepo({ collection: COLLECTION.listingProfile, action })).toBe(false);
			// The root keeps a performer's people.
			expect(permissions.allowsSpace({ ...performer, action, collection: COLLECTION.person })).toBe(false);
		}

		// locallibrary makes and removes sets; the site only edits them.
		expect(permissions.allowsRepo({ collection: COLLECTION.djSet, action: 'update' })).toBe(true);
		expect(permissions.allowsRepo({ collection: COLLECTION.djSet, action: 'create' })).toBe(false);
		expect(permissions.allowsRepo({ collection: COLLECTION.djSet, action: 'delete' })).toBe(false);
		expect(permissions.allowsSpace({ ...performer, action: 'read_self' })).toBe(true);
		expect(permissions.allowsSpace({ ...performer, action: 'read' })).toBe(false);
	});

	it('lets users upload avatars and mix tracklists to their own PDS, and nothing else', () => {
		// The PDS checks uploadBlob against the token's scopes like this.
		for (const scope of [full, base]) {
			const permissions = new ScopePermissionsTransition(scope);

			for (const mime of ['image/png', 'image/jpeg', 'image/webp', 'application/json'])
				expect(permissions.allowsBlob({ mime })).toBe(true);
			expect(permissions.allowsBlob({ mime: 'text/html' })).toBe(false);
		}
	});

	it('has a base scope without spaces for fallback, which a full grant covers', () => {
		expect(base.split(' ').some((scope) => scope.startsWith('include:') || scope.startsWith('space'))).toBe(false);
		expect(grantsAll(base, full, user)).toBe(false);
		expect(grantsAll(granted.join(' '), base, user)).toBe(true);
		expect(grantsAll(full, full, user)).toBe(true);
		expect(grantsAll(null, full, user)).toBe(false);
		expect(SCOPES.minimal).toBe('atproto');
	});

	it('lets any account keep a profile, follow, and announce shows as calendar events, with or without spaces', () => {
		for (const scopes of [granted, base.split(' ')]) {
			const permissions = new ScopePermissions(scopes);

			for (const action of ['create', 'update', 'delete'] as const)
				expect(permissions.allowsRepo({ collection: COLLECTION.profile, action })).toBe(true);

			for (const action of ['create', 'delete'] as const)
				expect(permissions.allowsRepo({ collection: COLLECTION.follow, action })).toBe(true);
			expect(permissions.allowsRepo({ collection: COLLECTION.follow, action: 'update' })).toBe(false);

			for (const collection of [COLLECTION.show, CALENDAR_EVENT])
				for (const action of ['create', 'update', 'delete'] as const)
					expect(permissions.allowsRepo({ collection, action })).toBe(true);
			expect(permissions.allowsRepo({ collection: COLLECTION.epk, action: 'create' })).toBe(scopes === granted);
		}
	});

	it('counts a grant from before shows and follows as one with spaces, which lacks only those writes', () => {
		// What the PDS grants from the permission set as it was, or from one it hasn't fetched again yet.
		const publicRecords = (scope: string) => /localnetwork\.(show|follow)|calendar\.event/.test(scope);
		const before = granted.filter((scope) => !publicRecords(scope)).join(' ');

		expect(grantsSpaces(before, user)).toBe(true);
		expect(grantsPublicRecords(before, user)).toBe(false);
		expect(grantsSpaces(granted.join(' '), user)).toBe(true);
		expect(grantsPublicRecords(granted.join(' '), user)).toBe(true);
		expect(grantsSpaces(base, user)).toBe(false);
		expect(grantsPublicRecords(base, user)).toBe(true);
		expect(grantsSpaces(granted.filter((scope) => !scope.startsWith('blob')).join(' '), user)).toBe(false);
	});

	it('says which grants can save a profile and its picture, so the editor asks the others to sign in first', () => {
		expect(grantsProfile(granted.join(' '), user)).toBe(true);
		expect(grantsProfile(base, user)).toBe(true);
		expect(grantsProfile(beforeProfiles.join(' '), user)).toBe(false);
		expect(grantsProfile(granted.filter((scope) => !scope.startsWith('blob')).join(' '), user)).toBe(false);
		expect(grantsProfile(null, user)).toBe(false);
	});

	it('counts a grant from before profiles as one with spaces, so nobody is signed out at deploy', () => {
		const before = beforeProfiles.join(' ');

		// The app signs a spaces-capable account out when this is false for its stored grant.
		expect(grantsSpaces(before, user)).toBe(true);
		expect(grantsSpaces(`${before} account:email?action=read`, user)).toBe(true);
		// It lacks the new writes, so the first one that needs them asks for a fresh sign-in, which gets them all.
		expect(grantsAll(before, full, user)).toBe(false);
		expect(grantsPublicRecords(before, user)).toBe(false);

		// Every space write it had is still needed, drafts in a project's backstage included.
		const publicRecords = (scope: string) => /localnetwork\.(show|follow)|calendar\.event/.test(scope);

		for (const dropped of beforeProfiles.filter((scope) => !publicRecords(scope)))
			expect(grantsSpaces(beforeProfiles.filter((scope) => scope !== dropped).join(' '), user)).toBe(false);
	});

	it('declares every scope it may ask for in its client metadata', () => {
		const declared = DECLARED_SCOPES.split(' ');

		for (const scope of Object.values(SCOPES).flatMap((scopes) => scopes.split(' '))) expect(declared).toContain(scope);
		expect(new Set(declared).size).toBe(declared.length);
		expect(new ScopePermissions(declared).allowsRepo({ collection: CALENDAR_RSVP, action: 'create' })).toBe(true);
	});

	it('compares grants the way the PDS writes them, and spots any scope missing', () => {
		expect(grantsAll(granted.join(' '), full, user)).toBe(true);
		expect(grantsAll(granted.join(' '), full, 'did:plc:someoneelse')).toBe(false);

		for (const dropped of granted)
			expect(grantsAll(granted.filter((scope) => scope !== dropped).join(' '), full, user)).toBe(false);
	});

	it('asks separately to post bug reports to userinput.app, and for nothing more there', () => {
		const permissions = new ScopePermissions(SCOPES.bugReport.split(' '));
		const discussion = 'app.userinput.discussion';

		expect(permissions.allowsRepo({ collection: discussion, action: 'create' })).toBe(true);
		expect(permissions.allowsRepo({ collection: discussion, action: 'update' })).toBe(false);
		expect(permissions.allowsRepo({ collection: 'app.userinput.upvote', action: 'create' })).toBe(false);
		// Asking for it at sign-in would send every older session back to approve it.
		expect(grantsAll(SCOPES.bugReport, full, user)).toBe(false);
		expect(grantsAll(granted.join(' '), SCOPES.bugReport, user)).toBe(false);
		expect(grantsAll(`${granted.join(' ')} ${SCOPES.bugReport}`, full, user)).toBe(true);
	});

	it('asks separately to read the email address, in either form a PDS knows, without touching sign-in', () => {
		expect(DECLARED_SCOPES.split(' ')).toEqual(expect.arrayContaining([SCOPES.email, SCOPES.emailLegacy]));
		// The PDS writes the default action out.
		expect(grantsAll('atproto account:email?action=read', SCOPES.email, user)).toBe(true);
		expect(grantsAll(SCOPES.emailLegacy, SCOPES.email, user)).toBe(false);
		expect(new ScopePermissionsTransition([SCOPES.emailLegacy]).allowsAccount({ attr: 'email', action: 'read' })).toBe(
			true
		);
		// An older grant without it still signs in, and one with it counts the same.
		expect(grantsSpaces(granted.join(' '), user)).toBe(true);
		expect(grantsSpaces(`${granted.join(' ')} ${SCOPES.email}`, user)).toBe(true);
		expect(grantsAll(SCOPES.email, full, user)).toBe(false);
	});

	it('finds the optional scopes a grant holds, in the form it holds them', () => {
		expect(heldOptionalScopes(granted.join(' '), user)).toEqual([]);
		expect(heldOptionalScopes(null, user)).toEqual([]);
		expect(heldOptionalScopes(`${granted.join(' ')} ${SCOPES.bugReport} account:email?action=read`, user)).toEqual([
			SCOPES.bugReport,
			SCOPES.email
		]);
		expect(heldOptionalScopes(`atproto ${SCOPES.emailLegacy}`, user)).toEqual([SCOPES.emailLegacy]);
	});

	it('builds loopback metadata for local development and web metadata otherwise', () => {
		expect(isLoopback('http://127.0.0.1:5173')).toBe(true);
		expect(isLoopback('https://localnetwork.computer')).toBe(false);
		const local = clientMetadata(config('http://127.0.0.1:5173'), full);
		expect(local.client_id).toMatch(/^http:\/\/localhost/);
		const web = clientMetadata(config('https://localnetwork.computer'), full);
		expect(web).toMatchObject({
			client_id: 'https://localnetwork.computer/oauth-client-metadata.json',
			redirect_uris: ['https://localnetwork.computer/oauth/callback'],
			dpop_bound_access_tokens: true
		});
	});
});

describe('OAuthService.authorize', () => {
	const user = 'did:plc:user';
	const page = new URL('https://pds.test/oauth/authorize');

	const account = (overrides: Partial<SignedInAccount> = {}): SignedInAccount => ({
		did: user,
		grantedScope: SCOPES.full,
		spacesCapable: 1,
		publicWrites: 1,
		...overrides
	});

	/**
	 * The OAuth service over a PDS that answers with `error` (a refused scope, by default) for every scope `refused`
	 * matches, recording each scope asked for.
	 */
	const service = async (
		refused: (scope: string) => boolean,
		error = new Error('invalid_scope: unsupported scope')
	) => {
		const oauth = new OAuthService({
			config: config('http://127.0.0.1:5173'),
			db: await openDatabase(':memory:'),
			logger: silentLogger,
			fetch: offline
		});

		const asked: string[] = [];

		vi.spyOn(oauth.client, 'authorize').mockImplementation(async (_identifier, options) => {
			const scope = options?.scope ?? '';
			asked.push(scope);

			if (refused(scope)) throw error;

			return page;
		});

		return { oauth, asked };
	};

	it('falls back through the tiers on a first sign-in, and gives up on anything but a scope error', async () => {
		const { oauth, asked } = await service((scope) => scope.includes('include:'));

		expect(await oauth.authorize('alice.test', '{}')).toBe(page);
		expect(asked).toEqual([SCOPES.full, SCOPES.base]);

		const down = await service(() => true, new Error('fetch failed'));
		await expect(down.oauth.authorize('alice.test', '{}')).rejects.toThrow('fetch failed');
		expect(down.asked).toHaveLength(1);
	});

	it('asks again for the optional scopes an account holds, so signing in keeps them', async () => {
		const { oauth, asked } = await service((scope) => scope.includes('include:'));
		const holding = account({ grantedScope: `${SCOPES.base} ${SCOPES.bugReport} account:email?action=read` });

		await oauth.authorize('alice.test', '{}', { account: holding });
		expect(asked).toEqual([
			`${SCOPES.full} ${SCOPES.bugReport} ${SCOPES.email}`,
			`${SCOPES.base} ${SCOPES.bugReport} ${SCOPES.email}`
		]);
	});

	it('never lowers an account with spaces to get an optional scope, trying each form of it instead', async () => {
		const { oauth, asked } = await service((scope) => scope.includes('email'));

		await expect(
			oauth.authorize(user, '{}', {
				account: account({ grantedScope: `${SCOPES.full} ${SCOPES.bugReport}` }),
				optional: 'email'
			})
		).rejects.toThrow(OptionalScopeUnsupported);
		expect(asked).toEqual([
			`${SCOPES.full} ${SCOPES.bugReport} ${SCOPES.email}`,
			`${SCOPES.full} ${SCOPES.bugReport} ${SCOPES.emailLegacy}`
		]);
	});

	it('lets an account without spaces try for them while opting in, but no lower than its own tier', async () => {
		const base = account({ grantedScope: SCOPES.base, spacesCapable: 0 });
		const { oauth, asked } = await service((scope) => scope.includes('include:'));

		await oauth.authorize(user, '{}', { account: base, optional: 'bugReport' });
		expect(asked).toEqual([`${SCOPES.full} ${SCOPES.bugReport}`, `${SCOPES.base} ${SCOPES.bugReport}`]);

		const refusing = await service((scope) => scope.includes('userinput'));
		await expect(refusing.oauth.authorize(user, '{}', { account: base, optional: 'bugReport' })).rejects.toThrow(
			OptionalScopeUnsupported
		);
		expect(refusing.asked).toHaveLength(2);

		// Before the app asked for public records, nothing says whether the base scopes were granted.
		const unknown = await service(() => true);
		await expect(
			unknown.oauth.authorize(user, '{}', {
				account: account({ grantedScope: 'atproto', spacesCapable: 0, publicWrites: null }),
				optional: 'bugReport'
			})
		).rejects.toThrow(OptionalScopeUnsupported);
		expect(unknown.asked).toHaveLength(3);
	});
});

describe('OAuthService sign-in, given servers a DID document names', () => {
	const PDS = 'pds.rebind.localnetwork.computer';
	const ISSUER = 'auth.rebind.localnetwork.computer';
	const did = 'did:plc:visitor';
	let loopback: Loopback;

	beforeEach(async () => {
		loopback = await rebindToLoopback([PDS, ISSUER]);

		return async () => {
			vi.unstubAllGlobals();
			vi.restoreAllMocks();
			await loopback.close();
		};
	});

	/**
	 * Signs `did` in as /login does, with the app's guarded fetch, a DID document whose PDS is `pds`, and private
	 * addresses allowed or not.
	 */
	const signIn = async (pds: string, allowPrivateNetwork = false) => {
		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork,
			logger: silentLogger
		});

		const oauth = new OAuthService({
			config: config('https://localnetwork.computer'),
			db: await openDatabase(':memory:'),
			logger: silentLogger,
			fetch: identity.serviceFetch
		});

		vi.spyOn(oauth.client.identityResolver, 'resolve').mockResolvedValue({
			did,
			handle: 'visitor.localnetwork.computer',
			didDoc: { id: did, service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: pds }] }
		});

		// What came of it: the authorization page, or the innermost cause of the error (for a request fetch refused, why).
		return oauth.authorize(did, '{}').then(
			(page) => page.href,
			(error: Error) => {
				let cause = error;

				while (cause.cause instanceof Error) cause = cause.cause;

				return cause.message;
			}
		);
	};

	it('refuses a PDS whose name points at loopback when it connects, and never reaches it', async () => {
		expect(await signIn(`https://${PDS}:${loopback.port}`)).toBe('Hostname resolved to non-unicast address');
		expect(loopback.connections()).toBe(0);
	});

	it("refuses an authorization server, named in the PDS's metadata, whose name points at loopback", async () => {
		const real = globalThis.fetch;
		const pds = 'https://pds.localnetwork.computer';

		// The PDS answers from a public address; everything else goes out to the network.
		vi.stubGlobal('fetch', async (input: string | URL | Request, init?: RequestInit) => {
			const request = new Request(input, init);

			if (!request.url.startsWith(`${pds}/`)) return real(input, init);

			return Response.json({ resource: pds, authorization_servers: [`https://${ISSUER}:${loopback.port}`] });
		});

		expect(await signIn(pds)).toBe('Hostname resolved to non-unicast address');
		expect(loopback.connections()).toBe(0);
	});

	it('follows no redirect from the authorization server, even where private addresses are allowed', async () => {
		let inside = 0;
		let origin = '';

		// A PDS that is its own authorization server, and sends its pushed authorization requests somewhere else.
		const server = createServer((request, response) => {
			if (request.url === '/.well-known/oauth-protected-resource') {
				response.setHeader('content-type', 'application/json');
				response.end(JSON.stringify({ resource: origin, authorization_servers: [origin] }));
			} else if (request.url === '/.well-known/oauth-authorization-server') {
				response.setHeader('content-type', 'application/json');

				response.end(
					JSON.stringify({
						issuer: origin,
						authorization_endpoint: `${origin}/authorize`,
						token_endpoint: `${origin}/token`,
						pushed_authorization_request_endpoint: `${origin}/par`,
						require_pushed_authorization_requests: true,
						response_types_supported: ['code'],
						code_challenge_methods_supported: ['S256'],
						token_endpoint_auth_methods_supported: ['none', 'private_key_jwt'],
						token_endpoint_auth_signing_alg_values_supported: ['ES256'],
						dpop_signing_alg_values_supported: ['ES256'],
						client_id_metadata_document_supported: true
					})
				);
			} else if (request.url === '/par') {
				response.writeHead(307, { location: '/inside' }).end();
			} else {
				inside++;
				response.writeHead(201, { 'content-type': 'application/json' });
				response.end(JSON.stringify({ request_uri: 'urn:ietf:params:oauth:request_uri:x', expires_in: 60 }));
			}
		});

		await once(server.listen(0, '127.0.0.1'), 'listening');
		// SAFETY: a server listening on a TCP port reports an AddressInfo.
		origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;

		try {
			expect(await signIn(origin, true)).toBe('unexpected redirect');
			expect(inside).toBe(0);
		} finally {
			await new Promise((resolve) => server.close(resolve));
		}
	});
});
