import { IncludeScope, ScopePermissions, ScopePermissionsTransition, SpacePermission } from '@atproto/oauth-scopes';
import { describe, expect, it } from 'vitest';
import { COLLECTION, PERMISSIONS, SPACE_TYPE } from '#lib/nsid.ts';
import { loadConfig } from '../config';
import { PERMISSION_SET } from './lexicons';
import { clientMetadata, grantsAll, isLoopback, SCOPES } from './oauth';

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

	it('lets users upload avatars and mix tracklists to their own PDS, and nothing else', () => {
		// The PDS checks uploadBlob against the token's scopes like this.
		const permissions = new ScopePermissionsTransition(full);

		for (const mime of ['image/png', 'image/jpeg', 'image/webp', 'application/json'])
			expect(permissions.allowsBlob({ mime })).toBe(true);
		expect(permissions.allowsBlob({ mime: 'text/html' })).toBe(false);
	});

	it('has a base scope without spaces for fallback', () => {
		expect(base).toBe('atproto');
		expect(grantsAll(base, full, user)).toBe(false);
		expect(grantsAll(full, full, user)).toBe(true);
		expect(grantsAll(null, full, user)).toBe(false);
	});

	it('compares grants the way the PDS writes them, and spots any scope missing', () => {
		expect(grantsAll(granted.join(' '), full, user)).toBe(true);
		expect(grantsAll(granted.join(' '), full, 'did:plc:someoneelse')).toBe(false);

		for (const dropped of granted)
			expect(grantsAll(granted.filter((scope) => scope !== dropped).join(' '), full, user)).toBe(false);
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
