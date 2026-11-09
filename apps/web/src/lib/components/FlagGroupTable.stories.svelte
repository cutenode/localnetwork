<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { FlagGroupView } from '#lib/types.ts';
	import FlagGroupTable from './FlagGroupTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Flag groups', component: FlagGroupTable, tags: ['autodocs'] });

	const member = (handle: string) => ({ did: `did:plc:${handle}`, handle: `${handle}.test` });

	const groups: FlagGroupView[] = [
		{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs', description: 'People who asked to try things early', members: [member('milo'), member('ada')], flags: ['parityCanary'] },
		{ rkey: '3lzy2ji4nms3a', name: 'Friends', description: null, members: [member('pat')], flags: [] }
	];
</script>

<Story name="Groups" args={{ groups }} />

<Story name="Groups on a phone" args={{ groups }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="No groups" args={{ groups: [] }} />
