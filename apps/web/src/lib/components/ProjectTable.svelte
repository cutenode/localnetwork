<script lang="ts">
	import Avatar from '#lib/components/Avatar.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { ProjectView } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';

	/** One page of the staff console's project search. */
	let { projects }: { projects: ProjectView[] } = $props();
</script>

{#snippet name(project: ProjectView)}
	<div class="flex min-w-0 items-center gap-3">
		<Avatar src={project.avatarUrl} name={project.profile.name} size="sm" />
		<a class="min-w-0 font-medium text-fg no-underline [overflow-wrap:anywhere] hover:underline" href="/admin/projects/{project.slug}">{project.profile.name}</a>
	</div>
{/snippet}

{#if projects.length}
	<!-- Cells center on the avatar, which is taller than a line. -->
	<div class="[&_td]:align-middle!">
		<ResponsiveTable rows={projects} key={(project) => project.uri}>
			{#snippet head()}<th>Project</th><th>Location</th><th>Managers</th><th>Rev</th><th>Updated</th><th>Status</th>{/snippet}
			{#snippet row(project)}
				<td>{@render name(project)}</td>
				<td class="text-fg-muted">{project.profile.location.city}, {project.profile.location.country}</td>
				<td class="font-mono {project.managers.length ? '' : 'text-fg-subtle'}">{project.managers.length || 'none'}</td>
				<td class="font-mono">{project.revision}</td>
				<td class="whitespace-nowrap text-fg-muted">{formatDate(project.updatedAt)}</td>
				<td><StatusPill status={project.status} /></td>
			{/snippet}
			{#snippet card(project)}
				<div class="flex items-center justify-between gap-3">
					{@render name(project)}
					<StatusPill status={project.status} />
				</div>
				<p class="text-label text-fg-muted">
					{project.profile.location.city}, {project.profile.location.country} · {project.managers.length === 1 ? '1 manager' : `${project.managers.length || 'no'} managers`} · revision {project.revision} · updated {formatDate(project.updatedAt)}
				</p>
			{/snippet}
		</ResponsiveTable>
	</div>
{:else}
	<div class="card p-5 text-small text-fg-muted">No projects match these filters.</div>
{/if}
