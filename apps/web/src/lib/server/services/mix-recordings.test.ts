import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectMix, ProjectProfile, ProjectView } from '#lib/types.ts';
import { silentLogger } from '../logger';
import { PlyrClient } from '../plyr';
import { createTestServices, ROOT } from '../testing/core';
import { withRecordings } from './mix-recordings';
import { ProjectService } from './projects';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

const STRANGER = 'did:plc:stranger';

const CID = 'bafyreihrc5vnkuv5mfz3szvu4bfrtucsxvpu5ms7thsuyfwexmx5gpoyzm';

const OTHER_CID = 'bafyreiaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';

const track = (did: string, rkey = '3plyr') => ({ uri: `at://${did}/fm.plyr.track/${rkey}`, cid: CID });

const PLYR = 'https://plyr.fm/track/1567';

const SOUNDCLOUD = 'https://soundcloud.com/neon/tidal-hours';

/** plyr.fm's answer for a track that names `record` as its own. */
const named = (record: { uri: string | null; cid: string | null }) => () =>
	Response.json({ id: 1567, title: 'Tidal Hours', atproto_record_uri: record.uri, atproto_record_cid: record.cid });

/** A plyr.fm answering each track ID from `answers` and 404ing any other, counting what it's asked. */
function plyr(answers: Record<string, () => Response>) {
	const asked: string[] = [];

	const client = new PlyrClient(silentLogger, async (input) => {
		const { pathname } = new URL(input instanceof Request ? input.url : input);
		asked.push(pathname);

		return answers[pathname.replace('/tracks/', '')]?.() ?? new Response('{}', { status: 404 });
	});

	return { client, asked };
}

const profile = (mixes: ProjectMix[]): ProjectProfile => ({
	name: 'Neon Tide',
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE' },
	mixes
});

const mix = (url: string, extra: Partial<ProjectMix> = {}): ProjectMix => ({ title: 'Tidal Hours', url, ...extra });

describe('withRecordings', () => {
	it("points a plyr.fm mix at its track's record in a manager's repo", async () => {
		const { client } = plyr({ 1567: named(track(DJ)) });

		const linked = await withRecordings(profile([mix(PLYR), mix(SOUNDCLOUD)]), undefined, [DJ], client);

		expect(linked.mixes).toEqual([mix(PLYR, { recording: track(DJ) }), mix(SOUNDCLOUD)]);
	});

	it("links no record by someone else, of another kind, or that plyr.fm can't name", async () => {
		for (const record of [
			track(STRANGER),
			{ uri: `at://${DJ}/app.bsky.feed.post/3plyr`, cid: CID },
			{ uri: track(DJ).uri, cid: 'not a cid' },
			{ uri: track(DJ).uri, cid: null }
		].map(named)) {
			const { client } = plyr({ 1567: record });

			expect((await withRecordings(profile([mix(PLYR)]), undefined, [DJ], client)).mixes).toEqual([mix(PLYR)]);
		}
	});

	it("keeps a link's published recording without asking plyr.fm, unless its manager left", async () => {
		const { client, asked } = plyr({ 1567: named({ ...track(DJ), cid: OTHER_CID }) });
		const published = profile([mix(PLYR, { recording: track(DJ) })]);

		expect((await withRecordings(profile([mix(PLYR, { title: 'Renamed' })]), published, [DJ], client)).mixes).toEqual([
			mix(PLYR, { title: 'Renamed', recording: track(DJ) })
		]);
		expect(asked).toEqual([]);

		expect((await withRecordings(profile([mix(PLYR)]), published, [STRANGER], client)).mixes).toEqual([mix(PLYR)]);
	});

	it("links none when plyr.fm can't answer", async () => {
		const { client } = plyr({ 1567: () => new Response('down', { status: 500 }) });

		expect((await withRecordings(profile([mix(PLYR)]), undefined, [DJ], client)).mixes).toEqual([mix(PLYR)]);
	});

	it('decides every recording itself, whatever the profile it gets says', async () => {
		const { client, asked } = plyr({});
		const claimed = profile([mix(SOUNDCLOUD, { recording: track(DJ) }), mix(PLYR, { recording: track(DJ) })]);

		expect((await withRecordings(claimed, undefined, [DJ], client)).mixes).toEqual([mix(SOUNDCLOUD), mix(PLYR)]);
		expect((await withRecordings(claimed, undefined, [DJ], undefined)).mixes).toEqual([mix(SOUNDCLOUD), mix(PLYR)]);
		expect(asked).toEqual(['/tracks/1567']);
	});

	it('leaves a profile without mixes alone', async () => {
		const { mixes: _none, ...bare } = profile([]);

		expect(await withRecordings(bare, undefined, [DJ], plyr({}).client)).toBe(bare);
	});
});

describe('publishing a plyr.fm mix', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
		project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile([]), managers: [DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it("writes the track's record into the project's record", async () => {
		const projects = new ProjectService(t.core, t.directory, 0, undefined, plyr({ 1567: named(track(DJ)) }).client);

		const { project: published, changes } = await projects.publish({
			existing: project,
			profile: profile([mix(PLYR)]),
			managers: [DJ],
			status: 'active',
			createdBy: ADMIN
		});

		expect(published.profile.mixes).toEqual([mix(PLYR, { recording: track(DJ) })]);
		expect(t.net.records.get(`${ROOT}|${COLLECTION.project}|${project.rkey}`)?.value).toMatchObject({
			profile: { mixes: [{ url: PLYR, recording: track(DJ) }] }
		});
		expect(changes.map((change) => change.field)).toContain('profile.mixes[0].recording');
	});
});
