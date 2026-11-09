<script lang="ts">
	import type { FlagGroupView } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';

	/** Every flag group. Each opens on its own page, where it's changed or deleted. */
	let { groups }: { groups: FlagGroupView[] } = $props();

	const accounts = (n: number) => `${n} ${n === 1 ? 'account' : 'accounts'}`;
</script>

{#snippet who(group: FlagGroupView)}
	<a class="link" href="/admin/flags/groups/{group.rkey}">{group.name}</a>
	{#if group.description}<p class="mt-1 text-label text-fg-muted">{group.description}</p>{/if}
{/snippet}

{#snippet used(group: FlagGroupView)}
	{#if group.flags.length === 0}<span class="text-fg-muted">No flags</span>{:else}<span class="font-mono text-label">{group.flags.join(', ')}</span>{/if}
{/snippet}

{#if groups.length === 0}
	<p class="card p-5 text-fg-muted">No groups yet.</p>
{:else}
	<ResponsiveTable rows={groups} key={(group) => group.rkey} from="xl">
		{#snippet head()}<th>Group</th><th>Accounts</th><th>Used by</th>{/snippet}
		{#snippet row(group)}
			<td>{@render who(group)}</td>
			<td class="whitespace-nowrap text-fg-muted">{group.members.length}</td>
			<td>{@render used(group)}</td>
		{/snippet}
		{#snippet card(group)}
			<div>{@render who(group)}</div>
			<p class="text-label text-fg-muted">{accounts(group.members.length)} · {@render used(group)}</p>
		{/snippet}
	</ResponsiveTable>
{/if}
