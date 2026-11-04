import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IdentityResolver } from '../atproto/identity';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { AccountService } from './accounts';

const DJ = 'did:plc:dj';

const login = { handle: 'dj.test', pdsUrl: 'https://pds.test', grantedScope: 'atproto' };

describe('AccountService', () => {
	let db: Db;

	beforeEach(async () => {
		db = await openDatabase(':memory:');
	});
	afterEach(async () => {
		await db.destroy();
	});

	/** A resolver that knows every DID but `unknown`, with a handle from the DID's last part. */
	const resolver = (unknown?: string) => {
		const identity = new IdentityResolver({
			plcUrl: 'https://plc.test',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		vi.spyOn(identity, 'resolve').mockImplementation(async (did) => {
			if (did === unknown) throw new Error('no such DID');

			return {
				did,
				handle: `${did.split(':')[2]}.test`,
				pds: 'https://pds.test',
				signingKey: 'did:key:z',
				kemKey: null
			};
		});

		return identity;
	};

	it('records what each sign-in granted, the latest one winning', async () => {
		const accounts = new AccountService(db, silentLogger);

		await accounts.recordLogin({ did: DJ, ...login, spacesCapable: true, publicWrites: true });
		expect(await accounts.get(DJ)).toMatchObject({ handle: 'dj.test', spacesCapable: 1, publicWrites: 1 });
		expect(await accounts.spacesCapable(DJ)).toBe(true);

		await accounts.recordLogin({ did: DJ, ...login, handle: 'neon.test', spacesCapable: false, publicWrites: false });
		expect(await accounts.get(DJ)).toMatchObject({ handle: 'neon.test', spacesCapable: 0, publicWrites: 0 });
		expect(await accounts.spacesCapable(DJ)).toBe(false);

		await accounts.recordLogin({ did: DJ, ...login, spacesCapable: true, publicWrites: true });
		expect(await accounts.get(DJ)).toMatchObject({ spacesCapable: 1, publicWrites: 1 });
		expect(await accounts.spacesCapable('did:plc:nobody')).toBe(false);
	});

	it('shows accounts it has never seen without a handle, and resolves none without a resolver', async () => {
		const accounts = new AccountService(db, silentLogger);

		expect(await accounts.view(DJ)).toEqual({ did: DJ, handle: null });
		expect(await accounts.view('')).toEqual({ did: '', handle: null });
		await accounts.learn([DJ]);
		expect(await accounts.get(DJ)).toBeUndefined();
	});

	it('caches the handles it resolves, at most 25 at a time, skipping DIDs that fail', async () => {
		const accounts = new AccountService(db, silentLogger, resolver('did:plc:unknown'));
		const many = Array.from({ length: 30 }, (_, index) => `did:plc:dj${index}`);

		await accounts.learn(['did:plc:unknown', ...many]);

		const stored = await db.selectFrom('account').select(['did', 'handle']).execute();
		expect(stored).toHaveLength(24);
		expect(stored[0]).toEqual({ did: 'did:plc:dj0', handle: 'dj0.test' });
		expect(await accounts.get('did:plc:unknown')).toBeUndefined();
	});

	it("updates the handle of an account that's already known", async () => {
		const accounts = new AccountService(db, silentLogger, resolver());
		await accounts.recordLogin({ did: DJ, ...login, handle: null, spacesCapable: true, publicWrites: true });

		await accounts.learn([DJ]);

		expect(await accounts.get(DJ)).toMatchObject({ handle: 'dj.test', pdsUrl: 'https://pds.test', spacesCapable: 1 });
	});
});
