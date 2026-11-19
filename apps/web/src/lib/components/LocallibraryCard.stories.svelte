<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { LocallibraryStatus } from '#lib/dj.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import LocallibraryCard from './LocallibraryCard.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/locallibrary card', component: LocallibraryCard, tags: ['autodocs'] });

	const ago = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

	const set = { href: '/@neon.test/sets/3kset', name: 'Tidal Hours at Halle Nord', at: ago(3) };

	const published: LocallibraryStatus = { statsAt: ago(50), sets: set };
</script>

<Story name="Loading" args={{ status: null }} />

<Story name="Published" args={{ status: published }} />

<Story name="Stats only" args={{ status: { statsAt: ago(50), sets: null } }} />

<Story name="Sets only" args={{ status: { statsAt: null, sets: set } }} />

<!-- No set says when it was made, so there's no date to show. -->
<Story name="Published, no date" args={{ status: { statsAt: null, sets: { ...set, at: null } } }} />

<Story name="Nothing published" args={{ status: { statsAt: null, sets: null } }} />

<Story name="Phone" args={{ status: { ...published, sets: { ...set, name: 'Tidal Hours closing set at Halle Nord, a name that runs past the edge of a phone' } } }} globals={phone} />
