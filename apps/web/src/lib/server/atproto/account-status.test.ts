import { describe, expect, it } from 'vitest';
import { silentLogger } from '../logger';
import { type AccountStatus, AccountStatuses, repoStatusLookup } from './account-status';
import type { ResolvedIdentity } from './identity';
import { writtenLog } from '../testing/written-log';

const MARA = 'did:plc:mara';

const OSKAR = 'did:plc:oskar';

const PDS = 'https://pds.test';

const NEW_PDS = 'https://new-pds.test';

const HALF_HOUR = 30 * 60_000;

describe('repoStatusLookup', () => {
	it('should read an active account as active from the PDS its DID document names', async () => {
		// Arrange
		const lookup = repoStatusLookup(pdsAnswering(MARA, Response.json({ did: MARA, active: true })));

		// Act
		const status = await lookup(MARA);

		// Assert
		expect(status).toBe('active');
	});

	it.each(['deactivated', 'takendown', 'suspended'] as const)('should read a %s account as such', async (inactive) => {
		const lookup = repoStatusLookup(pdsAnswering(MARA, Response.json({ did: MARA, active: false, status: inactive })));

		expect(await lookup(MARA)).toBe(inactive);
	});

	it('should read an account its PDS no longer has as deleted', async () => {
		const answer = Response.json({ error: 'RepoNotFound', message: 'Could not find repo' }, { status: 400 });

		expect(await repoStatusLookup(pdsAnswering(MARA, answer))(MARA)).toBe('deleted');
	});

	it('should read a sync condition as active, since the account is still there', async () => {
		const answer = Response.json({ did: MARA, active: false, status: 'desynchronized' });

		expect(await repoStatusLookup(pdsAnswering(MARA, answer))(MARA)).toBe('active');
	});

	it.each([
		['RepoNotFound', Response.json({ error: 'RepoNotFound', message: 'Could not find repo' }, { status: 400 })],
		['deactivated', Response.json({ did: MARA, active: false, status: 'deactivated' })]
	])(
		'should ask the PDS an account moved to when the one its cached DID document names answers %s',
		async (_, answer) => {
			// Arrange
			const lookup = repoStatusLookup(movedAccount(MARA, answer));

			// Act
			const status = await lookup(MARA);

			// Assert
			expect(status).toBe('active');
		}
	);

	it("should throw when the PDS can't say", async () => {
		const lookup = repoStatusLookup(pdsAnswering(MARA, new Response('down', { status: 502 })));

		await expect(lookup(MARA)).rejects.toThrow('HTTP 502');
	});
});

describe('AccountStatuses', () => {
	it("should ask an account's PDS once, and reuse the answer for half an hour", async () => {
		// Arrange
		const pds = new Map<string, AccountStatus>([[MARA, 'deactivated']]);
		const { statuses, clock } = tracked(pds);
		await statuses.status(MARA);
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR - 1;

		// Act
		await statuses.refresh();

		// Assert
		expect(await statuses.status(MARA)).toBe('deactivated');
	});

	it('should answer with the last status while asking again once it is old', async () => {
		const pds = new Map<string, AccountStatus>([[MARA, 'deactivated']]);
		const { statuses, clock } = tracked(pds);
		await statuses.status(MARA);
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR;

		expect(await statuses.status(MARA)).toBe('deactivated');
		// The lookup here only awaits promises, so the next turn of the event loop finds it done.
		await new Promise((resolve) => setImmediate(resolve));
		expect(await statuses.status(MARA)).toBe('active');
	});

	it("should take an account it never saw as active when its PDS can't be asked", async () => {
		const { statuses } = tracked(new Map());

		expect(await statuses.status(OSKAR)).toBe('active');
	});

	it("should keep an account as last seen when its PDS can't be asked", async () => {
		const pds = new Map<string, AccountStatus>([[MARA, 'deactivated']]);
		const { statuses, clock } = tracked(pds, { accounts: async () => [MARA] });
		await statuses.status(MARA);
		pds.delete(MARA);
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect(await statuses.status(MARA)).toBe('deactivated');
	});

	it('should ask about a deleted account again once its answer is old, as an account that just moved can look deleted', async () => {
		const pds = new Map<string, AccountStatus>([[MARA, 'deleted']]);
		const { statuses, clock } = tracked(pds);
		await statuses.status(MARA);
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect(await statuses.status(MARA)).toBe('active');
	});

	it.each(['takendown', 'suspended'] as const)('should say when a %s account is active again', async (inactive) => {
		const pds = new Map<string, AccountStatus>([[MARA, inactive]]);
		const reinstated: string[] = [];
		const { statuses, clock } = tracked(pds, { reinstated: (did) => reinstated.push(did) });
		await statuses.refresh();
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect(reinstated).toEqual([MARA]);
	});

	it('should not say a deactivated account is reinstated, since its records were kept', async () => {
		const pds = new Map<string, AccountStatus>([[MARA, 'deactivated']]);
		const reinstated: string[] = [];
		const { statuses, clock } = tracked(pds, { reinstated: (did) => reinstated.push(did) });
		await statuses.refresh();
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect(reinstated).toEqual([]);
	});

	it("should list none of the directory's accounts as inactive until the first check is done", () => {
		const { statuses } = tracked(new Map([[MARA, 'deactivated']]));

		expect(statuses.inactive()).toEqual([]);
	});

	it("should list the directory's inactive accounts once checked, and drop one that's active again", async () => {
		const pds = new Map<string, AccountStatus>([
			[MARA, 'deactivated'],
			[OSKAR, 'active']
		]);

		const { statuses, clock } = tracked(pds);
		await statuses.refresh();
		const before = statuses.inactive();
		pds.set(MARA, 'active');
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect({ before, after: statuses.inactive() }).toEqual({ before: [MARA], after: [] });
	});

	it("should leave deleted accounts out of the directory's inactive list, since HappyView dropped their records", async () => {
		const pds = new Map<string, AccountStatus>([
			[MARA, 'deleted'],
			[OSKAR, 'deactivated']
		]);

		const { statuses } = tracked(pds);

		await statuses.refresh();

		expect(statuses.inactive()).toEqual([OSKAR]);
	});

	it('should send the directory queries at most 100 inactive accounts, saying how many there were', async () => {
		const pds = new Map<string, AccountStatus>(
			Array.from({ length: 101 }, (_, index) => [`did:plc:inactive${index}`, 'deactivated'] as const)
		);

		const { logger, notes } = writtenLog('warn');
		const { statuses } = tracked(pds, { logger });

		await statuses.refresh();

		expect(statuses.inactive()).toHaveLength(100);
		expect(notes()).toContainEqual(
			expect.objectContaining({ message: 'more inactive accounts than the directory queries take', inactive: 101 })
		);
	});

	it("should keep the last list when the directory can't list its accounts", async () => {
		const pds = new Map<string, AccountStatus>([[MARA, 'deactivated']]);
		let down = false;

		const { statuses, clock } = tracked(pds, {
			accounts: async () => {
				if (down) throw new Error('directory down');

				return [...pds.keys()];
			}
		});

		await statuses.refresh();
		down = true;
		clock.now = HALF_HOUR;

		await statuses.refresh();

		expect(statuses.inactive()).toEqual([MARA]);
	});
});

// Helpers

/** An identity resolver whose every account is on PDS, which answers `did`'s getRepoStatus with `answer`. */
function pdsAnswering(did: string, answer: Response) {
	return {
		resolve: async (asked: string): Promise<ResolvedIdentity> => ({
			did: asked,
			handle: null,
			pds: PDS,
			signingKey: 'did:key:z',
			kemKey: null
		}),
		serviceApiFetch: async (input: string | URL | Request) => {
			const url = new URL(input instanceof Request ? input.url : input);
			const asks = url.origin === PDS && url.pathname === '/xrpc/com.atproto.sync.getRepoStatus';

			return asks && url.searchParams.get('did') === did ? answer : new Response('not found', { status: 404 });
		}
	};
}

/**
 * An account that moved from PDS to NEW_PDS, which has it active, while the resolver's cached DID document still names
 * PDS, which answers `before`. Only a forced refresh reads the current document.
 */
function movedAccount(did: string, before: Response) {
	return {
		resolve: async (asked: string, forceRefresh = false): Promise<ResolvedIdentity> => ({
			did: asked,
			handle: null,
			pds: forceRefresh ? NEW_PDS : PDS,
			signingKey: 'did:key:z',
			kemKey: null
		}),
		serviceApiFetch: async (input: string | URL | Request) => {
			const url = new URL(input instanceof Request ? input.url : input);

			if (url.searchParams.get('did') !== did) return new Response('not found', { status: 404 });

			return url.origin === PDS ? before : Response.json({ did, active: true });
		}
	};
}

/**
 * AccountStatuses over `pds` (an account it lacks can't be asked), with the directory's accounts being the ones in it
 * unless `options` says otherwise, and a clock that only moves when a test moves it.
 */
function tracked(
	pds: Map<string, AccountStatus>,
	options: Partial<ConstructorParameters<typeof AccountStatuses>[0]> = {}
) {
	const clock = { now: 0 };

	const statuses = new AccountStatuses({
		lookup: async (did) => {
			const status = pds.get(did);

			if (!status) throw new Error(`no PDS answers for ${did}`);

			return status;
		},
		accounts: async () => [...pds.keys()],
		logger: silentLogger,
		now: () => clock.now,
		...options
	});

	return { statuses, clock };
}
