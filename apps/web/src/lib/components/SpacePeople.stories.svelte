<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import type { SpacePerson } from '#lib/types.ts';
	import SpacePeople from './SpacePeople.svelte';

	const { Story } = defineMeta({ title: 'Staff/Space people', component: SpacePeople, tags: ['autodocs'] });

	const roles = ['host', 'cohost', 'door'];

	const person = (did: string, handle: string | null, role: string, capabilities: SpacePerson['capabilities'], hidden = false): SpacePerson => ({
		did,
		handle,
		role,
		capabilities,
		hidden,
		via: 'invite',
		addedBy: null,
		createdAt: '2026-10-20T10:00:00Z',
		updatedAt: '2026-10-20T10:00:00Z'
	});

	const people = [
		person('did:plc:neon', 'neon.test', 'host', ['page.edit', 'people.manage', 'events.manage', 'guests.manage']),
		person('did:plc:harbour', 'harbour.test', 'cohost', ['page.edit', 'events.manage'], true),
		person('did:plc:door', null, 'door', ['guests.checkin'])
	];

	const pending = {
		invites: [{ did: 'did:plc:pirate', handle: 'pirate.test', role: 'door', createdAt: '2026-10-21T10:00:00Z', expiresAt: '2026-11-20T10:00:00Z' }],
		requests: [{ did: 'did:plc:fan', handle: 'fan.test', message: 'I can help at the door', createdAt: '2026-10-22T10:00:00Z' }],
		links: 1
	};

	const nothing = { invites: [], requests: [], links: 0 };

	/** A venue's people: its stored `booker` role reads Booking manager. */
	const venue = {
		people: [
			person('did:plc:tresor', 'tresor.test', 'owner', ['page.edit', 'people.manage', 'venue.approve', 'venue.block']),
			person('did:plc:desk', 'desk.test', 'booker', ['venue.approve'])
		],
		pending: { invites: [{ did: 'did:plc:night', handle: 'night.test', role: 'booker', createdAt: '2026-10-21T10:00:00Z', expiresAt: null }], requests: [], links: 0 },
		roles: ['owner', 'booker']
	};
</script>

<Story name="People and who's waiting" args={{ people, pending, roles }} />

<Story name="People and who's waiting on a phone" args={{ people, pending, roles }} globals={phone} />

<Story name="A venue's people" args={venue} />

<Story
	name="Someone who can't do anything yet, and plain invites"
	args={{
		people: [person('did:plc:door', 'door.test', 'door', [])],
		pending: {
			invites: [{ ...pending.invites[0], expiresAt: null }],
			requests: [{ ...pending.requests[0], message: null }],
			links: 2
		},
		roles
	}}
/>

<Story name="Loading"args={{ people: new Promise<SpacePerson[]>(() => {}), pending: new Promise<typeof nothing>(() => {}), roles }} />

<Story name="Nobody in it" args={{ people: [], pending: nothing, roles }} />

<Story name="Couldn't load" args={{ people: null, pending: null, roles }} />
