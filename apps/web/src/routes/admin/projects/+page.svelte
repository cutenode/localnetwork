<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Projects · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	{#snippet add()}<a href="/admin/projects/new" class="btn-primary"><Plus class="size-4" />Add a project</a>{/snippet}
	<SectionHeader level={1} title="Projects" actions={data.viewer?.isAdmin ? add : undefined} />

	<form method="GET" class="flex flex-wrap items-center gap-2">
		<input name="q" class="input w-full sm:max-w-xs" value={data.q} placeholder="Search projects" aria-label="Search projects" />
		<select name="status" class="input w-auto min-w-40" aria-label="Status">
			<option value="">Any status</option>
			{#each ['active', 'hidden', 'archived'] as status (status)}<option value={status} selected={data.status === status}>{status}</option>{/each}
		</select>
		<label class="flex min-h-11 items-center gap-2 px-1 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="unclaimed" checked={data.unclaimed} />Unclaimed only</label>
		<button class="btn-secondary">Filter</button>
	</form>

	<div class="card overflow-x-auto">
		<table class="table-base min-w-180">
			<thead><tr><th>Project</th><th>Location</th><th>Managers</th><th>Rev</th><th>Updated</th><th>Status</th></tr></thead>
			<tbody class="[&_td]:align-middle!">
				{#each data.items as project (project.uri)}
					<tr>
						<td>
							<div class="flex items-center gap-3">
								<Avatar src={project.avatarUrl} name={project.profile.name} size="sm" />
								<a class="font-medium text-fg no-underline hover:underline" href="/admin/projects/{project.slug}">{project.profile.name}</a>
							</div>
						</td>
						<td class="text-fg-muted">{project.profile.location.city}, {project.profile.location.country}</td>
						<td class="font-mono {project.managers.length ? '' : 'text-fg-subtle'}">{project.managers.length || 'none'}</td>
						<td class="font-mono">{project.revision}</td>
						<td class="whitespace-nowrap text-fg-muted">{formatDate(project.updatedAt)}</td>
						<td><StatusPill status={project.status} /></td>
					</tr>
				{:else}
					<tr><td colspan="6" class="py-8 text-center text-fg-muted">No projects match these filters.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<Pagination page={data.page} pageSize={data.pageSize} total={data.total} />
</div>
