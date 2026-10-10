import { ScopePermissionsTransition, SpacePermission } from '@atproto/oauth-scopes';
import { describe, expect, it } from 'vitest';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import { loadConfig } from '../config';
import { buildScopes, clientMetadata, grantsAll, isLoopback } from './oauth';

const config = (publicUrl: string) =>
	loadConfig({
		ROOT_IDENTIFIER: 'did:plc:root',
		ROOT_PASSWORD: 'x',
		PUBLIC_URL: publicUrl,
		HAPPYVIEW_URL: 'http://127.0.0.1:8000'
	});

describe('OAuth scopes', () => {
	const { full, base } = buildScopes('did:plc:root');
	const user = 'did:plc:user';

	it('requests granular space scopes that parse with the official parser', () => {
		const spaces = full.split(' ').filter((scope) => scope.startsWith('space:'));
		expect(spaces).toHaveLength(4);

		for (const scope of spaces) expect(SpacePermission.fromString(scope)).not.toBeNull();
		const audit = SpacePermission.fromString(spaces[0])!;
		expect(
			audit.matches({
				type: 'computer.localnetwork.space.audit',
				authority: 'did:plc:root',
				skey: 'main',
				action: 'create',
				collection: 'computer.localnetwork.audit.entry'
			})
		).toBe(true);
		expect(
			audit.matches({
				type: 'computer.localnetwork.space.audit',
				authority: 'did:plc:root',
				skey: 'main',
				action: 'read'
			})
		).toBe(false);
		const planner = SpacePermission.fromString(spaces[3])!;
		expect(planner.authority).toBe('self');
		expect(planner.manage).toEqual(['create', 'update']);
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
		// As the PDS returns them: the planner's `self` authority resolved and its default actions dropped.
		const granted = full
			.split(' ')
			.map((scope) =>
				scope.startsWith(`space:${SPACE_TYPE.planner}`)
					? `space:${SPACE_TYPE.planner}?authority=${user}&skey=self&collection=${COLLECTION.plannerSave}&collection=${COLLECTION.plannerShow}&manage=create&manage=update`
					: scope
			);

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
