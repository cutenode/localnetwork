import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile, RequestRecord } from '#lib/types.ts';
import { ForbiddenError, NotFoundError } from '../core';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const PROMOTER = 'did:plc:promoter';

const LEGACY = 'did:plc:legacy';

// Decisions carry the request's CID, which the lexicon checks is one.
const CID = 'bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]);

const profile = (name: string, overrides: Partial<ProjectProfile> = {}): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
	...overrides
});

describe('RequestService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ, PROMOTER] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const nowish = () => new Date().toISOString();

	/** The URI a request PROMOTER wrote to the moderation space would have. */
	const requestUri = (rkey: string) => `${t.spaces.moderation}/${PROMOTER}/${COLLECTION.request}/${rkey}`;

	/** Indexes a request record as sync would bring it from PROMOTER's repo, skipping the checks `submit` makes. */
	const synced = async (rkey: string, value: Omit<RequestRecord, 'createdAt'> & { createdAt?: string }) => {
		const uri = requestUri(rkey);
		await t.requests.index({ uri, cid: CID, repoDid: PROMOTER, value: { createdAt: nowish(), ...value } });

		return uri;
	};

	const create = async (name: string, by = DJ) =>
		t.requests.submit({ actor: await t.viewer(by), kind: 'create', profile: profile(name) });

	const auditSummaries = async (action: string) => {
		await t.audit.idle();

		return (await t.audit.list({ action })).map((entry) => entry.summary);
	};

	describe('submit', () => {
		it('refuses claims from managers, on archived projects, and twice over', async () => {
			const admin = await t.viewer(ADMIN);
			const dj = await t.viewer(DJ);
			const claim = { relationship: 'self', evidence: 'It is me' };
			const managed = await t.projectAdmin.create({ actor: admin, profile: profile('Mine'), managers: [DJ] });
			await expect(t.requests.submit({ actor: dj, kind: 'claim', projectUri: managed.uri, claim })).rejects.toThrow(
				'You already manage this project'
			);

			const old = await t.projectAdmin.create({ actor: admin, profile: profile('Old') });
			await t.projectAdmin.setStatus({ actor: admin, project: old, status: 'archived' });
			await expect(t.requests.submit({ actor: dj, kind: 'claim', projectUri: old.uri, claim })).rejects.toThrow(
				'Archived projects cannot be claimed'
			);

			const open = await t.projectAdmin.create({ actor: admin, profile: profile('Open') });
			const first = await t.requests.submit({ actor: dj, kind: 'claim', projectUri: open.uri, claim });
			expect(first).toMatchObject({ kind: 'claim', status: 'pending', record: { claim } });
			await expect(t.requests.submit({ actor: dj, kind: 'claim', projectUri: open.uri, claim })).rejects.toThrow(
				'You already have a claim waiting for review on this project'
			);
		});

		it('needs a change in an update, and records none for a details-only one', async () => {
			const dj = await t.viewer(DJ);

			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Same'),
				managers: [DJ]
			});

			await expect(t.requests.submit({ actor: dj, kind: 'update', projectUri: project.uri })).rejects.toThrow(
				'Nothing to change'
			);

			const update = await t.requests.submit({
				actor: dj,
				kind: 'update',
				projectUri: project.uri,
				details: { bookingEmail: 'new@b.test' }
			});

			expect(update.record.profile).toBeUndefined();
			expect(update.record.details).toEqual({ bookingEmail: 'new@b.test' });
			await t.audit.idle();
			const [entry] = await t.audit.list({ action: 'request.submit', subject: update.uri });
			expect(JSON.parse(entry.changes ?? '[]')).toEqual([]);
		});

		it("keeps blobs of the writer's own in a proposed profile as they are", async () => {
			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Fresh'),
				managers: [DJ]
			});

			const own = await t.net.client(DJ).uploadBlob(PNG, 'image/png');

			const update = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'update',
				projectUri: project.uri,
				profile: { ...project.profile, avatar: own }
			});

			expect(update.record.profile?.avatar?.ref.$link).toBe(own.ref.$link);
		});

		it('attaches tracklists only to the mixes they line up with', async () => {
			const tracklist = { bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' };

			const request = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'create',
				profile: profile('Aligned', {
					mixes: [
						{ title: 'Plain', url: 'https://mixes.test/plain' },
						{ title: 'Listed', url: 'https://mixes.test/listed' }
					]
				}),
				tracklists: [undefined, tracklist, tracklist]
			});

			const mixes = request.record.profile?.mixes;
			expect(mixes?.map((mix) => mix.tracklist?.mimeType)).toEqual([undefined, 'application/json']);
			expect(request.record.profile?.avatar).toBeUndefined();
		});

		it('refuses to copy a published avatar without a blob fetcher', async () => {
			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Pictured'),
				avatar: { bytes: PNG, mimeType: 'image/png' },
				managers: [DJ]
			});

			t.core.fetchBlob = undefined;
			await expect(
				t.requests.submit({
					actor: await t.viewer(DJ),
					kind: 'update',
					projectUri: project.uri,
					profile: { ...project.profile, tagline: 'Same face' }
				})
			).rejects.toThrow('No blob fetcher configured to copy published files');
		});

		it('fails loudly when the request it just wrote cannot be read back', async () => {
			vi.spyOn(t.requests, 'get').mockResolvedValueOnce(null);
			await expect(create('Ghost')).rejects.toThrow('Request vanished after submitting');
		});
	});

	describe('revise', () => {
		it('lets only the requester revise, and only an undecided request', async () => {
			const request = await create('Mine');
			await expect(
				t.requests.revise(await t.viewer(PROMOTER), request.uri, { message: 'hijack' })
			).rejects.toBeInstanceOf(ForbiddenError);
			await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			await expect(t.requests.revise(await t.viewer(DJ), request.uri, { message: 'late' })).rejects.toThrow(
				'This request has already been decided'
			);
			await expect(t.requests.revise(await t.viewer(DJ), requestUri('missing'), {})).rejects.toBeInstanceOf(
				NotFoundError
			);
		});

		it('answers a "changes requested" decision with new claim evidence and a message', async () => {
			const dj = await t.viewer(DJ);
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Claimed') });

			const request = await t.requests.submit({
				actor: dj,
				kind: 'claim',
				projectUri: project.uri,
				claim: { relationship: 'self', evidence: 'Trust me' }
			});

			const asked = await t.requests.decide({
				actor: await t.viewer(MOD),
				uri: request.uri,
				outcome: 'changesRequested',
				reason: ' Link your socials '
			});

			expect(asked).toMatchObject({ status: 'changes_requested', decision: { reason: 'Link your socials' } });

			const revised = await t.requests.revise(dj, request.uri, {
				claim: { relationship: 'self', evidence: 'See my site', links: ['https://dj.test'] },
				message: ' Added a link '
			});

			expect(revised).toMatchObject({
				status: 'pending',
				projectUri: project.uri,
				record: { claim: { evidence: 'See my site' }, message: 'Added a link' }
			});
			expect(await auditSummaries('request.changesRequested')).toEqual([
				"Requested changes to did:plc:dj's claim for Claimed"
			]);
		});

		it("replaces an update's private details", async () => {
			const dj = await t.viewer(DJ);

			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Private'),
				managers: [DJ]
			});

			const request = await t.requests.submit({
				actor: dj,
				kind: 'update',
				projectUri: project.uri,
				details: { bookingEmail: 'a@b.test' }
			});

			const revised = await t.requests.revise(dj, request.uri, { details: { bookingEmail: 'c@d.test' } });
			expect(revised.record.details).toEqual({ bookingEmail: 'c@d.test' });
		});

		it('asks the requester to sign in again when their repo holds the request but the app has no session', async () => {
			const request = await create('Lapsed');
			t.capable.delete(DJ);
			await expect(t.requests.revise(await t.viewer(DJ), request.uri, { message: 'hi' })).rejects.toThrow(
				'Sign in again to edit this request'
			);
		});

		it('revises and withdraws a request filed on behalf through the root', async () => {
			const legacy = await t.viewer(LEGACY);
			const request = await create('Proxy', LEGACY);

			const revised = await t.requests.revise(legacy, request.uri, { profile: profile('Proxy', { tagline: 'New' }) });
			expect(revised).toMatchObject({ repoDid: ROOT, record: { onBehalfOf: LEGACY, profile: { tagline: 'New' } } });
			await t.requests.withdraw(legacy, request.uri);
			expect((await t.requests.get(request.uri))?.status).toBe('withdrawn');
			const written = t.net.spaceRecordsIn(t.spaces.moderation, ROOT).find((record) => record.uri === request.uri);
			expect(written?.value).toMatchObject({ onBehalfOf: LEGACY, withdrawnAt: expect.any(String) });
		});
	});

	describe('decide', () => {
		it('denies and records reviews by non-staff and by the requester, whatever the outcome', async () => {
			const dj = await t.viewer(DJ);
			const mod = await t.viewer(MOD);
			const theirs = await create('Theirs', PROMOTER);
			const own = await create('Own', MOD);

			await expect(t.requests.decide({ actor: dj, uri: theirs.uri, outcome: 'approved' })).rejects.toThrow(
				new ForbiddenError('Only moderators and admins can review requests')
			);
			await expect(
				t.requests.decide({ actor: dj, uri: theirs.uri, outcome: 'changesRequested', reason: 'x' })
			).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.requests.decide({ actor: mod, uri: own.uri, outcome: 'rejected', reason: 'x' })).rejects.toThrow(
				new ForbiddenError("You can't review your own request")
			);

			await t.audit.idle();
			const denied = await t.audit.list({ outcome: 'denied' });
			expect(denied.map((entry) => [entry.action, entry.actorDid, entry.reason]).toReversed()).toEqual([
				['request.approve', DJ, 'Only moderators and admins can review requests'],
				['request.changesRequested', DJ, 'Only moderators and admins can review requests'],
				['request.reject', MOD, "You can't review your own request"]
			]);
			expect((await t.requests.get(theirs.uri))?.status).toBe('pending');
		});

		it("won't approve a listing request once the requester's profile is gone", async () => {
			const uri = await synced('3list', {
				kind: 'list',
				listing: { subject: `at://${PROMOTER}/${COLLECTION.profile}/self` }
			});

			await expect(t.requests.decide({ actor: await t.viewer(MOD), uri, outcome: 'approved' })).rejects.toThrow(
				'Their profile is gone'
			);
			await t.requests.decide({ actor: await t.viewer(MOD), uri, outcome: 'rejected', reason: 'No profile' });
			expect(await auditSummaries('request.reject')).toEqual([
				"Rejected did:plc:promoter's listing request for their profile"
			]);
		});

		it('names the requester by handle once the app knows it', async () => {
			await t.accounts.recordLogin({
				did: DJ,
				handle: 'dj.test',
				pdsUrl: null,
				grantedScope: 'atproto',
				spacesCapable: true,
				publicWrites: false
			});
			const request = await create('Handled');
			await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			expect(await auditSummaries('request.approve')).toEqual(["Approved @dj.test's new project for Handled"]);
		});

		it("won't publish a synced create without a profile, and names it plainly", async () => {
			const uri = await synced('3bare', { kind: 'create' });
			const mod = await t.viewer(MOD);
			await expect(t.requests.decide({ actor: mod, uri, outcome: 'approved' })).rejects.toThrow(
				'This request has no profile to publish'
			);
			await t.requests.decide({ actor: mod, uri, outcome: 'rejected', reason: 'Empty' });
			expect(await auditSummaries('request.reject')).toEqual(["Rejected did:plc:promoter's new project for a project"]);
		});

		it('refuses to approve a claim on a project that no longer exists', async () => {
			const uri = await synced('3gone', {
				kind: 'claim',
				project: `at://${ROOT}/${COLLECTION.project}/3missing`,
				claim: { relationship: 'self', evidence: 'x' }
			});

			await expect(t.requests.decide({ actor: await t.viewer(MOD), uri, outcome: 'approved' })).rejects.toThrow(
				'The project no longer exists'
			);
		});

		it('refuses an update from someone who stopped managing the project', async () => {
			const admin = await t.viewer(ADMIN);
			const project = await t.projectAdmin.create({ actor: admin, profile: profile('Moved'), managers: [DJ] });

			const update = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'update',
				projectUri: project.uri,
				profile: profile('Moved', { tagline: 'Late' })
			});

			await t.projectAdmin.setManagers({ actor: admin, project, managers: [] });
			await expect(
				t.requests.decide({ actor: await t.viewer(MOD), uri: update.uri, outcome: 'approved' })
			).rejects.toThrow('The requester no longer manages this project');
		});

		it('refuses a claim that would give a project more than 10 managers', async () => {
			const managers = Array.from({ length: 10 }, (_, index) => `did:plc:m${index}`);
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Full'), managers });

			const claim = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'claim',
				projectUri: project.uri,
				claim: { relationship: 'self', evidence: 'x' }
			});

			await expect(
				t.requests.decide({ actor: await t.viewer(MOD), uri: claim.uri, outcome: 'approved' })
			).rejects.toThrow('A project can have at most 10 managers');
		});
	});

	describe('copying files on approval', () => {
		it("keeps an on-behalf request's files, which the root already holds", async () => {
			const request = await t.requests.submit({
				actor: await t.viewer(LEGACY),
				kind: 'create',
				profile: profile('Rooted'),
				avatar: { bytes: PNG, mimeType: 'image/png' }
			});

			await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			const project = await t.projects.bySlug('rooted');
			expect(project?.profile.avatar?.ref.$link).toBe(request.record.profile?.avatar?.ref.$link);
		});

		it("takes the avatar's proposed type when the requester's PDS serves it without one", async () => {
			const request = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'create',
				profile: profile('Untyped'),
				avatar: { bytes: PNG, mimeType: 'image/png' }
			});

			const cid = request.record.profile!.avatar!.ref.$link;
			t.net.blobs.set(cid, { bytes: PNG, mimeType: '' });
			await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			expect((await t.projects.bySlug('untyped'))?.profile.avatar?.mimeType).toBe('image/png');
		});

		it('publishes without the avatar and tracklists when no space blob fetcher is configured', async () => {
			const request = await t.requests.submit({
				actor: await t.viewer(DJ),
				kind: 'create',
				profile: profile('Bare', { mixes: [{ title: 'Set', url: 'https://mixes.test/set' }] }),
				avatar: { bytes: PNG, mimeType: 'image/png' },
				tracklists: [{ bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' }]
			});

			expect(request.record.profile?.mixes?.[0]?.tracklist).toBeDefined();
			t.core.fetchSpaceBlob = undefined;
			await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			const project = await t.projects.bySlug('bare');
			expect(project?.profile.avatar).toBeUndefined();
			expect(project?.profile.mixes).toEqual([{ title: 'Set', url: 'https://mixes.test/set' }]);
		});
	});

	describe('index', () => {
		it('ignores a URI that is not a space record', async () => {
			await t.requests.index({
				uri: 'at://did:plc:x/app.bsky.feed.post/1',
				cid: 'c',
				repoDid: PROMOTER,
				value: { kind: 'create', createdAt: nowish() }
			});
			expect(await t.requests.list()).toEqual([]);
		});

		it('reopens a request sent back for changes only when a later edit syncs', async () => {
			const uri = await synced('3back', { kind: 'create', createdAt: '2026-01-01T00:00:00.000Z' });

			await t.requests.applyDecision({
				uri: `${t.spaces.moderation}/${ROOT}/${COLLECTION.decision}/3d`,
				value: {
					request: uri,
					requester: PROMOTER,
					outcome: 'changesRequested',
					reason: 'More',
					decidedBy: MOD,
					createdAt: '2026-01-02T00:00:00.000Z'
				}
			});

			const resync = (updatedAt?: string) =>
				synced('3back', { kind: 'create', createdAt: '2026-01-01T00:00:00.000Z', updatedAt });

			const status = async () => (await t.requests.get(uri))?.status;
			await resync();
			expect(await status()).toBe('changes_requested');
			await resync('2026-01-01T12:00:00.000Z');
			expect(await status()).toBe('changes_requested');
			await resync('2026-01-03T00:00:00.000Z');
			expect(await status()).toBe('pending');
		});
	});

	describe('applyDecision', () => {
		const decisionUri = (rkey: string) => `${t.spaces.moderation}/${ROOT}/${COLLECTION.decision}/${rkey}`;

		const decision = (request: string, outcome: string, createdAt: string) => ({
			request,
			requester: PROMOTER,
			outcome,
			decidedBy: MOD,
			createdAt
		});

		it('skips malformed decisions, unknown requests, older decisions, and withdrawn requests', async () => {
			const uri = await synced('3apply', { kind: 'create', profile: profile('Applied') });

			await t.requests.applyDecision({ uri: decisionUri('bad'), value: { request: uri, outcome: 'maybe' } });
			expect((await t.requests.get(uri))?.status).toBe('pending');

			await t.requests.applyDecision({
				uri: decisionUri('elsewhere'),
				value: decision(requestUri('unknown'), 'approved', '2026-01-02T00:00:00.000Z')
			});
			expect(await t.requests.get(requestUri('unknown'))).toBeNull();

			await t.requests.applyDecision({
				uri: decisionUri('new'),
				value: decision(uri, 'rejected', '2026-01-03T00:00:00.000Z')
			});
			await t.requests.applyDecision({
				uri: decisionUri('old'),
				value: decision(uri, 'approved', '2026-01-02T00:00:00.000Z')
			});
			expect(await t.requests.get(uri)).toMatchObject({
				status: 'rejected',
				decision: { at: '2026-01-03T00:00:00.000Z', by: { did: MOD } }
			});

			const withdrawn = await create('Withdrawn');
			await t.requests.withdraw(await t.viewer(DJ), withdrawn.uri);
			await t.requests.applyDecision({
				uri: decisionUri('late'),
				value: decision(withdrawn.uri, 'approved', '2099-01-01T00:00:00.000Z')
			});
			expect((await t.requests.get(withdrawn.uri))?.status).toBe('withdrawn');
		});

		it('shows no decider for a decision that names none', async () => {
			const uri = await synced('3anon', { kind: 'create', profile: profile('Anon') });

			await t.requests.applyDecision({
				uri: decisionUri('anon'),
				value: { ...decision(uri, 'rejected', '2026-01-02T00:00:00.000Z'), decidedBy: '' }
			});

			expect((await t.requests.get(uri))?.decision).toMatchObject({ by: null, at: '2026-01-02T00:00:00.000Z' });
		});
	});

	describe('reads', () => {
		it('lists by exact status, oldest first when pending, and finds nothing for an unknown URI', async () => {
			const on = (rkey: string, day: number) =>
				synced(rkey, { kind: 'create', profile: profile(rkey), createdAt: `2026-01-0${day}T00:00:00.000Z` });

			const first = await on('3a', 1);
			const second = await on('3b', 2);
			const third = await on('3c', 3);
			await t.requests.withdraw(await t.viewer(PROMOTER), second);

			expect((await t.requests.list({ status: 'pending' })).map((request) => request.uri)).toEqual([first, third]);
			expect((await t.requests.list()).map((request) => request.uri)).toEqual([third, second, first]);
			expect((await t.requests.list({ status: 'withdrawn' })).map((request) => request.uri)).toEqual([second]);
			expect(await t.requests.get(requestUri('nope'))).toBeNull();
			await expect(t.requests.withdraw(await t.viewer(PROMOTER), requestUri('nope'))).rejects.toBeInstanceOf(
				NotFoundError
			);
		});
	});

	describe('deletedAt', () => {
		it('is null while the record is there, and when it was deleted once a decided one is gone', async () => {
			const legacy = await t.viewer(LEGACY);
			const request = await create('Proxy', LEGACY);
			await t.requests.decide({ actor: await t.viewer(ADMIN), uri: request.uri, outcome: 'rejected', reason: 'No' });
			expect((await t.requests.get(request.uri))?.deletedAt).toBeNull();

			const before = nowish();
			await t.heldRecords.delete(legacy, request.uri);

			const deletedAt = (await t.requests.get(request.uri))?.deletedAt;
			expect(deletedAt! >= before && deletedAt! <= nowish()).toBe(true);
			expect((await t.requests.list({ requester: LEGACY })).map((view) => view.deletedAt)).toEqual([deletedAt]);
		});
	});
});
