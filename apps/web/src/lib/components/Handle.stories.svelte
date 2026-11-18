<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { NEON } from '#lib/testing/profiles.ts';
	import Handle from './Handle.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Handle', component: Handle, tags: ['autodocs'] });

	const neon = { did: NEON, handle: 'neon.test' };
</script>

<Story name="Linked" args={{ account: neon }} />

<Story name="Plain" args={{ account: neon, link: false }} />

<Story name="No handle" args={{ account: { did: 'did:plc:pat3hk8sz5qw2n7vdc4xm6jr', handle: null } }} />

<Story name="Unknown account" args={{ account: null }} />
