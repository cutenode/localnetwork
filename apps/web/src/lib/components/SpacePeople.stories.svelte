<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
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
</script>

<Story name="People and who's waiting" args={{ people, pending, roles }} />

<Story name="People and who's waiting on a phone" args={{ people, pending, roles }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Loading" args={{ people: new Promise<SpacePerson[]>(() => {}), pending: new Promise<typeof nothing>(() => {}), roles }} />

<Story name="Nobody in it" args={{ people: [], pending: nothing, roles }} />

<Story name="Couldn't load" args={{ people: null, pending: null, roles }} />
