import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

let t: Awaited<ReturnType<typeof createTestServices>>;

let venue: VenueView;

beforeEach(async () => {
	t = await createTestServices({ capable: [ADA, BO] });
	venue = await t.venues.add(await t.viewer(BO), { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' } });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

const on: Partial<Flags> = { peopleSpaces: true };

/** The claim page for the venue at `key`, as Ada, with `flags`. */
const opened = async (key = venue.skey, flags = on) =>
	loadRoute<PageServerData>(load, {
		app: t,
		viewer: { ...(await t.viewer(ADA)), handle: 'ada.test' },
		url: `/dashboard/venues/${key}/claim`,
		params: { key },
		flags
	});

/** Files the claim form as Ada. */
const claim = async (form: Record<string, string>) =>
	callRoute(actions.default, {
		app: t,
		viewer: await t.viewer(ADA),
		url: `/dashboard/venues/${venue.skey}/claim`,
		params: { key: venue.skey },
		flags: on,
		form
	});

it('opens the claim form for a venue, with who is claiming it', async () => {
	expect(await opened()).toEqual({
		venue: { space: venue.space, name: 'Tresor', locality: 'Berlin' },
		claimant: { did: ADA, handle: 'ada.test' }
	});
});

it('is a 404 while peopleSpaces is off for the viewer, and for a venue that is not there', async () => {
	await expect(opened(venue.skey, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened('3nothere')).rejects.toMatchObject({ status: 404 });
});

it('files the claim as an owner and opens the request', async () => {
	const form = {
		relationship: 'owner',
		evidence: 'The venue site lists me.',
		links: 'tresor.example/team',
		message: 'Hi'
	};

	const submitted = claim(form);

	await expect(submitted).rejects.toMatchObject({
		status: 303,
		location: expect.stringMatching(/^\/dashboard\/account\/requests\/\w+\?submitted=1$/)
	});
	const [request] = await t.requests.list({ kind: 'claim' });
	expect(request.record).toMatchObject({
		space: venue.space,
		claim: { relationship: 'owner', evidence: 'The venue site lists me.', links: ['https://tresor.example/team'] },
		message: 'Hi'
	});
});

it('keeps what was typed and says what is wrong', async () => {
	expect(await claim({ relationship: 'member', evidence: 'x' })).toMatchObject({
		status: 400,
		data: { error: 'Choose how you are connected to the venue', values: { evidence: 'x' } }
	});
	expect(await claim({ relationship: 'owner', evidence: '' })).toMatchObject({
		status: 400,
		data: { error: 'Tell the moderators how to verify your claim' }
	});
});
