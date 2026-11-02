<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import ProjectTable from '#lib/components/ProjectTable.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const fields = $derived(formFields({ q: data.q, unclaimed: data.unclaimed }));
</script>

<svelte:head><title>Projects · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	{#snippet add()}<a href="/admin/projects/new" class="btn-primary"><Plus class="size-4" />Add a project</a>{/snippet}
	<SectionHeader level={1} title="Projects" actions={data.viewer?.isAdmin ? add : undefined} />

	<form method="GET" class="flex flex-wrap items-center gap-2">
		<input name="q" class="input w-full sm:max-w-xs" bind:value={fields.q} placeholder="Search projects" aria-label="Search projects" />
		<select name="status" class="input w-auto min-w-40" aria-label="Status">
			<option value="">Any status</option>
			{#each ['active', 'hidden', 'archived'] as status (status)}<option value={status} selected={data.status === status}>{status}</option>{/each}
		</select>
		<label class="flex min-h-11 items-center gap-2 px-1 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="unclaimed" bind:checked={fields.unclaimed} />Unclaimed only</label>
		<button class="btn-secondary">Filter</button>
	</form>

	<ProjectTable projects={data.items} />
	<Pagination page={data.page} pageSize={data.pageSize} total={data.total} />
</div>
