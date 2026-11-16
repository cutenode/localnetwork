import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import SpacePeople from './SpacePeople.svelte';

const desk = {
	did: 'did:plc:desk',
	handle: 'desk.test',
	role: 'booker',
	capabilities: ['venue.approve' as const],
	hidden: false,
	via: 'invite',
	addedBy: null,
	createdAt: '2026-10-20T10:00:00Z',
	updatedAt: '2026-10-20T10:00:00Z'
};

describe('SpacePeople', () => {
	it("names a venue's booker role Booking manager, in the role picker and its invites", async () => {
		await render(SpacePeople, {
			people: [desk],
			pending: {
				invites: [
					{
						did: 'did:plc:night',
						handle: 'night.test',
						role: 'booker',
						createdAt: '2026-10-21T10:00:00Z',
						expiresAt: null
					}
				],
				requests: [],
				links: 0
			},
			roles: ['owner', 'booker']
		});

		// The table and the phone cards each have one.
		const picker = page.getByRole('combobox', { name: 'Role for desk.test' }).first();
		await expect.element(picker).toHaveValue('booker');
		await expect.element(picker.getByRole('option', { name: 'Booking manager' })).toBeInTheDocument();
		await expect.element(page.getByText(/as booking manager/)).toBeVisible();
	});
});
