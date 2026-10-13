<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Projects · Staff</title></svelte:head>

<div class="flex flex-wrap items-center justify-between gap-4">
	<h1 class="text-3xl">Projects</h1>
	{#if data.viewer?.isAdmin}<a href="/admin/projects/new" class="btn-accent"><Plus class="size-4" />Add a project</a>{/if}
</div>
<form method="GET" class="mt-6 flex flex-wrap gap-2">
	<input name="q" class="input max-w-xs" value={data.q} placeholder="Search projects" />
	<select name="status" class="input w-40" aria-label="Status">
		<option value="">Any status</option>
		{#each ['active', 'hidden', 'archived'] as status (status)}<option value={status} selected={data.status === status}>{status}</option>{/each}
	</select>
	<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" name="unclaimed" checked={data.unclaimed} />Unclaimed only</label>
	<button class="btn-secondary">Filter</button>
</form>
<div class="card mt-6 overflow-x-auto">
	<table class="table-base">
		<thead><tr><th>Project</th><th>Location</th><th>Managers</th><th>Rev</th><th>Updated</th><th>Status</th></tr></thead>
		<tbody>
			{#each data.items as project (project.uri)}
				<tr>
					<td>
						<div class="flex items-center gap-3">
							<Avatar src={project.avatarUrl} name={project.profile.name} size="sm" />
							<a class="menu-link" href="/admin/projects/{project.slug}">{project.profile.name}</a>
						</div>
					</td>
					<td>{project.profile.location.city}, {project.profile.location.country}</td>
					<td>{project.managers.length || 'none'}</td>
					<td>{project.revision}</td>
					<td class="whitespace-nowrap">{formatDate(project.updatedAt)}</td>
					<td><StatusPill status={project.status} /></td>
				</tr>
			{:else}
				<tr><td colspan="6" class="muted py-8 text-center">No projects.</td></tr>
			{/each}
		</tbody>
	</table>
</div>
<Pagination page={data.page} pageSize={data.pageSize} total={data.total} />
