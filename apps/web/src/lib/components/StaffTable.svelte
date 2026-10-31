<script lang="ts">
	import { enhance } from '$app/forms';
	import Handle from '#lib/components/Handle.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { StaffMember } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';

	/** Everyone with a staff role. `viewer` can't remove their own, and nobody can remove one from `ADMIN_DIDS`. */
	let { staff, viewer }: { staff: StaffMember[]; viewer: string | undefined } = $props();
</script>

{#snippet who(member: StaffMember)}
	<Handle account={member} />
	<p class="font-mono text-label text-fg-subtle [overflow-wrap:anywhere]">{member.did}</p>
	{#if member.note}<p class="mt-1 text-label text-fg-muted">{member.note}</p>{/if}
{/snippet}

{#snippet granted(member: StaffMember)}
	{#if member.bootstrap}<span class="font-mono text-label">ADMIN_DIDS</span>{:else}{formatDate(member.createdAt)}{/if}
{/snippet}

{#snippet remove(member: StaffMember)}
	{#if !member.bootstrap && member.did !== viewer}
		<form method="POST" action="?/revoke" use:enhance={({ cancel }) => { if (!confirm('Remove this role?')) cancel(); }}>
			<input type="hidden" name="did" value={member.did} />
			<button class="btn-danger btn-sm">Remove</button>
		</form>
	{/if}
{/snippet}

<ResponsiveTable rows={staff} key={(member) => member.did} from="xl">
	{#snippet head()}<th>Account</th><th>Role</th><th>Granted</th><th><span class="sr-only">Actions</span></th>{/snippet}
	{#snippet row(member)}
		<td>{@render who(member)}</td>
		<td class="capitalize">{member.role}</td>
		<td class="whitespace-nowrap text-fg-muted">{@render granted(member)}</td>
		<td class="text-right">{@render remove(member)}</td>
	{/snippet}
	{#snippet card(member)}
		<div>{@render who(member)}</div>
		<p class="text-label text-fg-muted"><span class="capitalize">{member.role}</span> · {member.bootstrap ? '' : 'granted '}{@render granted(member)}</p>
		{@render remove(member)}
	{/snippet}
</ResponsiveTable>
