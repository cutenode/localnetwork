import { deepStrictEqual } from 'node:assert/strict';
import { AtUri } from '@atproto/syntax';
import PQueue from 'p-queue';
import { COLLECTION } from '#lib/nsid.ts';
import type { HappyViewBundle } from '../../src/lib/server/happyview/bundle.ts';
import type { DirectoryOutputs, HappyViewClient } from '../../src/lib/server/happyview/client.ts';
import { refreshTargets } from '../../src/lib/server/happyview/refresh.ts';
import { syncHappyView } from '../../src/lib/server/happyview/sync.ts';

type Name = keyof DirectoryOutputs;

type Params = NonNullable<Parameters<HappyViewClient['query']>[1]>;

/** The queries that answer a page at a time, with a `cursor` for the next. */
type Paged = 'searchProjects' | 'searchPerformers' | 'listShows' | 'listRecords';

/** One directory query to ask both instances. */
export type ParityCase = { name: Name; params: Params };

/** A query the two instances answered differently, with what differed. */
export type Mismatch = ParityCase & { diff: string };

/** How many queries or re-reads run at once on each instance. */
const CONCURRENCY = 4;

/** The most URIs or actors the directory's batch queries take (their lexicons' maxLength). */
const URIS = 25;

const ACTORS = 10;

const chunks = <T>(items: T[], size: number): T[][] =>
	Array.from({ length: Math.ceil(items.length / size) }, (_, index) => items.slice(index * size, (index + 1) * size));

const unique = <T>(items: T[]): T[] => [...new Set(items)];

/** Every page of a paged query, as the cases that ask for each page and `client`'s answers to them. */
async function walk<K extends Paged>(client: HappyViewClient, name: K, params: Params) {
	const cases: ParityCase[] = [];
	const answers: DirectoryOutputs[K][] = [];
	const seen = new Set<string>();
	let cursor: string | undefined;

	do {
		const page = cursor ? { ...params, cursor } : params;
		cases.push({ name, params: page });
		answers.push(await client.query(name, page));

		if (cursor) seen.add(cursor);
		cursor = answers.at(-1)!.cursor;
	} while (cursor && !seen.has(cursor));

	return { cases, answers };
}

/** Every project `client` knows, in any status, and the accounts that manage them. */
async function projects(client: HappyViewClient) {
	const walked = await walk(client, 'searchProjects', { includeInactive: true, sort: 'name', limit: 60 });
	const found = walked.answers.flatMap((page) => page.projects);
	// SAFETY: the project lexicon requires `managers`, an array of DIDs, and the directory only serves valid records.
	const managers = unique(found.flatMap((project) => project.record.managers as string[]));

	return { ...walked, uris: found.map((project) => project.uri), managers };
}

/**
 * The queries to compare: every computer.localnetwork.directory.* query, asked about the root and about the projects,
 * DJs, shows, sets, artists and records `old` knows (each page of the paged ones), and each record collection in
 * `collections` for every one of those accounts.
 */
export async function parityCases(old: HappyViewClient, rootDid: string, collections: string[]): Promise<ParityCase[]> {
	const found = await projects(old);
	const performers = await walk(old, 'searchPerformers', { sort: 'name', limit: 60 });
	const shows = await walk(old, 'listShows', { limit: 100 });

	const djs = performers.answers.flatMap((page) =>
		page.performers.flatMap((performer) => ('profile' in performer ? [new AtUri(performer.profile.uri).host] : []))
	);

	const accounts = unique([rootDid, ...found.managers, ...djs]);
	const actorChunks = chunks(accounts, ACTORS);
	const sets = await Promise.all(actorChunks.map((actors) => old.query('listSets', { actors, limit: 100 })));
	const { artists } = await old.query('searchArtists', { limit: 50 });

	const records = await Promise.all(
		accounts.flatMap((actor) =>
			collections.map((collection) => walk(old, 'listRecords', { actor, collection, limit: 100 }))
		)
	);

	const setUris = sets.flatMap((answer) => answer.sets.map((set) => set.uri));
	const showUris = shows.answers.flatMap((page) => page.shows.map((show) => show.uri));

	const recordUris = records.flatMap(({ answers }) =>
		answers.flatMap((page) => page.records.map((record) => record.uri))
	);

	return [
		{ name: 'getStats', params: {} },
		{ name: 'searchProjects', params: {} },
		{ name: 'matchProjects', params: { includeOutOfRange: true, limit: 50 } },
		{ name: 'getCharts', params: { limit: 100 } },
		{ name: 'searchArtists', params: { limit: 50 } },
		...found.cases,
		...performers.cases,
		...shows.cases,
		...found.uris.flatMap((uri): ParityCase[] => [
			{ name: 'getProject', params: { uri, counts: true } },
			{ name: 'getFollowStatus', params: { subject: uri } },
			{ name: 'listShows', params: { project: uri, limit: 100 } }
		]),
		...chunks(found.uris, URIS).map((uris): ParityCase => ({ name: 'getProjects', params: { uris } })),
		...actorChunks.map((actors): ParityCase => ({ name: 'listSets', params: { actors, limit: 100 } })),
		...chunks(setUris, URIS).map((uris): ParityCase => ({ name: 'getSets', params: { uris } })),
		...chunks(showUris, URIS).map((uris): ParityCase => ({ name: 'getShows', params: { uris } })),
		...artists.map((artist): ParityCase => ({
			name: 'getArtist',
			params: artist.mbid ? { mbid: artist.mbid } : { name: artist.name }
		})),
		...records.flatMap(({ cases }) => cases),
		...chunks(recordUris, URIS).map((uris): ParityCase => ({ name: 'getRecords', params: { uris } }))
	];
}

/** What a query answered, or the error it failed with, so a query failing on one instance only is a difference too. */
const answer = (client: HappyViewClient, { name, params }: ParityCase) =>
	client.query(name, params).catch((error) => ({ error: error instanceof Error ? error.message : String(error) }));

/** Asks `old` and `next` each case, and returns the ones they answered differently. */
export async function compare(cases: ParityCase[], old: HappyViewClient, next: HappyViewClient): Promise<Mismatch[]> {
	const queue = new PQueue({ concurrency: CONCURRENCY });

	const results = await Promise.all(
		cases.map((parityCase) =>
			queue.add(async (): Promise<Mismatch | null> => {
				const [before, after] = await Promise.all([answer(old, parityCase), answer(next, parityCase)]);

				try {
					deepStrictEqual(after, before);

					return null;
				} catch (error) {
					// SAFETY: deepStrictEqual throws an AssertionError, whose message is the diff.
					return { ...parityCase, diff: (error as Error).message };
				}
			})
		)
	);

	return results.filter((result) => result !== null && result !== undefined);
}

/**
 * Brings `next` in step with this app the way it does on boot: uploads the bundle's lexicons and scripts and re-reads
 * the root's projects (syncHappyView), then re-reads what refreshTargets lists and every manager's shows, a collection
 * per job (per 500 accounts), and waits up to `waitMs` for each job. The managers stand in for the follow readers too:
 * a manager whose press kits don't read follows has their follows dropped by the follow script. Returns how many jobs
 * ended in each status, with a collection HappyView turned away counted once, as its error.
 */
export async function syncLikeTheApp(
	next: HappyViewClient,
	bundle: HappyViewBundle,
	options: { rootDid: string; userinputSpace?: string; waitMs: number }
): Promise<Record<string, number>> {
	await syncHappyView(next, bundle, options.rootDid);
	const { managers } = await projects(next);
	const fromManagers = async () => managers;

	const targets = [
		...refreshTargets(options.rootDid, {
			managers: fromManagers,
			followReaders: fromManagers,
			userinputSpace: options.userinputSpace
		}),
		{ collection: COLLECTION.show, accounts: fromManagers }
	];

	const queue = new PQueue({ concurrency: CONCURRENCY });

	const statuses = await Promise.all(
		targets.map(({ collection, accounts }) =>
			queue.add(async () => {
				try {
					const jobs = await next.backfill(collection, await accounts());

					return await Promise.all(
						jobs.map(async (job) => (await next.awaitBackfill(job, options.waitMs)) ?? 'unknown')
					);
				} catch (error) {
					return [`error: ${error instanceof Error ? error.message : String(error)}`];
				}
			})
		)
	);

	return Object.fromEntries([...Map.groupBy(statuses.flat(), String)].map(([status, all]) => [status, all.length]));
}
