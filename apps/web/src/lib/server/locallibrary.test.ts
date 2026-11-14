import { expect, it } from 'vitest';
import type { DjActivity, DjSet } from '#lib/dj.ts';
import { locallibraryStatus } from './locallibrary.ts';

const set = (rkey: string, date: string, createdAt?: string): DjSet => ({
	uri: `at://did:plc:neon/computer.localnetwork.dj.set/${rkey}`,
	title: `Set ${rkey}`,
	date,
	tracks: [],
	createdAt
});

/** The status from `activity`, as DjService's cache would hand it over. */
const status = (activity: DjActivity[]) =>
	locallibraryStatus(
		{ app: { dj: { fresh: () => activity, activity: async () => activity } } },
		{ did: 'did:plc:neon', handle: null }
	);

it("leaves out a set's publish time it couldn't read, and says when none can be", async () => {
	const activity = (sets: DjSet[]): DjActivity[] => [{ did: 'did:plc:neon', handle: null, profile: null, sets }];

	expect(
		(await status(activity([set('a', '2026-11-07'), set('b', '2026-11-01', '2026-11-02T10:00:00Z')]))).sets
	).toEqual({
		href: '/@did:plc:neon/sets/a',
		name: 'Set a',
		at: '2026-11-02T10:00:00Z'
	});
	expect((await status(activity([set('a', '2026-11-07')]))).sets?.at).toBeNull();
});
