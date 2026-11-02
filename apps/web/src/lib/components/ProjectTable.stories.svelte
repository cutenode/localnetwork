<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ProjectView } from '#lib/types.ts';
	import ProjectTable from './ProjectTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Projects', component: ProjectTable, tags: ['autodocs'] });

	const project = (slug: string, name: string, city: string, managers: string[], status: ProjectView['status'], revision: number): ProjectView => ({
		uri: `at://did:plc:root/computer.localnetwork.project/${slug}`,
		rkey: slug,
		cid: 'c',
		slug,
		profile: { name, kind: 'solo', genres: ['techno'], location: { city, country: 'DE' } },
		managers,
		status,
		revision,
		createdBy: 'did:plc:ada',
		createdAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-10-20T10:00:00Z',
		avatarUrl: null
	});

	const projects = [
		project('neon-tide', 'Neon Tide', 'Berlin', ['did:plc:neon'], 'active', 4),
		project('harbour-lights', 'Harbour Lights', 'Hamburg', ['did:plc:harbour', 'did:plc:door'], 'hidden', 2),
		project('pirate-signal', 'Pirate Signal Sound System', 'Leipzig', [], 'archived', 1)
	];
</script>

<Story name="Projects" args={{ projects }} />

<Story name="Projects on a phone" args={{ projects }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="No matches" args={{ projects: [] }} />
