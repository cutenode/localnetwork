import { page } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { stubFormAction } from '#lib/testing/form-action.ts';
import type { IssuedVerification, SpacePerson, StaffMember } from '#lib/types.ts';
import ChannelTopics from './ChannelTopics.svelte';
import FollowButton from './FollowButton.svelte';
import ProfileSetupForm from './ProfileSetupForm.svelte';
import SpacePeople from './SpacePeople.svelte';
import StaffTable from './StaffTable.svelte';
import VerificationTable from './VerificationTable.svelte';

afterEach(() => {
	// Spies first, since one may wrap the stubbed fetch.
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
});

describe('FollowButton', () => {
	it('holds the button while a follow is on its way, then lets it go', async () => {
		const server = stubFormAction();
		await render(FollowButton, { follow: { followers: 12, following: false } });

		await page.getByRole('button', { name: /Follow/ }).click();
		await expect.element(page.getByRole('button', { name: /Follow/ })).toBeDisabled();

		server.answer();
		await expect.element(page.getByRole('button', { name: /Follow/ })).toBeEnabled();
	});
});

describe('ProfileSetupForm', () => {
	it('sends the name as typed, saying so until the server answers', async () => {
		const server = stubFormAction();
		await render(ProfileSetupForm, {
			data: { profile: null, prefill: { name: 'neon.test', hasAvatar: false }, listing: false, follows: false }
		});

		await page.getByLabelText('Name').fill('Neon Tide');
		await page.getByRole('button', { name: 'Create profile' }).click();
		await expect.element(page.getByRole('button', { name: 'Creating…' })).toBeDisabled();
		await expect.poll(() => server.sent.map((body) => body.get('name'))).toEqual(['Neon Tide']);

		server.answer();
		await expect.element(page.getByRole('button', { name: 'Create profile' })).toBeEnabled();
	});
});

describe('ChannelTopics', () => {
	it('posts every ticked topic as soon as a box is ticked, and keeps the boxes as they were', async () => {
		const server = stubFormAction();
		await render(ChannelTopics, {
			topics: ['bookings', 'guests', 'events'],
			channels: [
				{ channel: 'email', view: { status: 'active', pausedReason: null, address: null, topics: ['bookings'] } }
			]
		});

		await page.getByLabelText('Guests by Email').click();
		await expect.poll(() => server.sent.map((body) => body.getAll('topic'))).toEqual([['bookings', 'guests']]);
		server.answer();

		await expect.element(page.getByLabelText('Guests by Email')).toBeChecked();
		await expect.element(page.getByLabelText('Events by Email')).not.toBeChecked();
	});
});

describe('Removing someone', () => {
	const person: SpacePerson = {
		did: 'did:plc:harbour',
		handle: 'harbour.test',
		role: 'door',
		capabilities: ['guests.checkin'],
		hidden: false,
		via: 'invite',
		addedBy: null,
		createdAt: '2026-10-20T10:00:00Z',
		updatedAt: '2026-10-20T10:00:00Z'
	};

	const verification: IssuedVerification = {
		uri: 'at://did:plc:root/app.bsky.graph.verification/3kver',
		cid: 'c',
		rkey: '3kver',
		subject: 'did:plc:harbour',
		handle: 'harbour.test',
		displayName: 'Harbour Lights',
		createdAt: '2026-10-12T10:00:00Z'
	};

	const moderator: StaffMember = {
		did: 'did:plc:harbour',
		handle: 'harbour.test',
		role: 'moderator',
		grants: [],
		grantedBy: 'did:plc:ada',
		note: null,
		createdAt: '2026-10-12T10:00:00Z',
		bootstrap: false
	};

	const tables = [
		{
			name: 'a person from a space',
			question: 'Remove them from it?',
			open: () => render(SpacePeople, { people: [person], pending: null, roles: ['host', 'door'] })
		},
		{
			name: 'a verification',
			question: 'Remove this verification?',
			open: () =>
				render(VerificationTable, {
					verifications: [verification],
					live: { [verification.subject]: { handle: 'harbour.test', displayName: 'Harbour Lights' } }
				})
		},
		{
			name: 'a staff role',
			question: 'Remove this role?',
			open: () => render(StaffTable, { staff: [moderator], viewer: 'did:plc:ada' })
		}
	];

	it.each(tables)('sends nothing when they back out of removing $name', async ({ question, open }) => {
		stubFormAction();
		// `sent` fills in a while after the post goes out, so this watches for the post itself.
		const post = vi.spyOn(globalThis, 'fetch');
		const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
		await open();

		await page.getByRole('button', { name: 'Remove' }).first().click();

		expect(confirm).toHaveBeenCalledWith(question);
		expect(post).not.toHaveBeenCalled();
	});

	it.each(tables)('sends who it is once they confirm removing $name', async ({ open }) => {
		const server = stubFormAction();
		vi.spyOn(window, 'confirm').mockReturnValue(true);
		await open();

		await page.getByRole('button', { name: 'Remove' }).first().click();

		await expect.poll(() => server.sent.map((body) => body.get('did'))).toEqual(['did:plc:harbour']);
		server.answer();
	});
});
