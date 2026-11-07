<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { StrayMember } from '#lib/types.ts';
	import StrayMembers from './StrayMembers.svelte';

	const { Story } = defineMeta({ title: 'Staff/Stray members', component: StrayMembers, tags: ['autodocs'] });

	const strays: StrayMember[] = [
		{ did: 'did:plc:mod7xq2kp4wz9d3c6bnhv5fe', handle: 'mod.test', spaces: ['audit', 'industry'] },
		{ did: 'did:plc:pat3hk8sz5qw2n7vdc4xm6jr', handle: null, spaces: ['audit', 'moderation', 'staff', 'industry'] }
	];
</script>

<Story name="Removals still going through" args={{ strays }} />

<Story name="Removals on a phone" args={{ strays }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Nobody left over" args={{ strays: [] }} />

<Story name="Couldn't check" args={{ strays: null }} />
