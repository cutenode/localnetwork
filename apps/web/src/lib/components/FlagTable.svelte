<script lang="ts">
	import { formatDate, formatShowDate } from '#lib/format.ts';
	import type { FlagView } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';

	/** Every flag in the catalog. Each opens on its own page, where it's changed. */
	let { flags }: { flags: FlagView[] } = $props();

	const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

	/** On for every signed-in account, which a release flag needs before signed-out visitors can have it. */
	const everyAccount = ({ everyone, percentage }: FlagView) => everyone || percentage === 100;

	/** Who the flag's gates take in, whether or not it's turned off. Empty when none do. */
	function gates(flag: FlagView): string {
		const { signedOut, percentage, groups, accounts } = flag;

		// Signed-out visitors only ever have it on top of every account.
		if (signedOut) return 'Everyone';

		if (everyAccount(flag)) return 'Every account';
		const parts = [percentage > 0 && `${percentage}%`, groups.length > 0 && count(groups.length, 'group', 'groups'), accounts.length > 0 && count(accounts.length, 'account', 'accounts')];

		return parts.filter(Boolean).join(' · ');
	}
</script>

{#snippet who(flag: FlagView)}
	<span class="flex flex-wrap items-center gap-2">
		<a class="link font-mono text-small" href="/admin/flags/{flag.key}">{flag.key}</a>
		{#if flag.readyToRemove}<span class="pill bg-warning/10 text-warning">Ready to remove</span>{/if}
		<!-- A calendar date, read in UTC like the flag's own page. -->
		{#if flag.removeBy}<span class="text-label text-fg-muted">Remove by {formatShowDate(flag.removeBy)}</span>{/if}
	</span>
	<p class="mt-1 text-label text-fg-muted">{flag.description}</p>
{/snippet}

{#snippet onFor(flag: FlagView)}
	{#if flag.enabled}
		<span>{gates(flag) || 'Off'}</span>
		{#if flag.type === 'release' && !flag.signedOut && everyAccount(flag)}<span class="block text-label text-fg-muted">Signed-out visitors next</span>{/if}
	{:else}
		<span class="font-medium text-critical">Off for everyone</span>
		{#if gates(flag)}<span class="block text-label text-fg-muted">Kept: {gates(flag)}</span>{/if}
	{/if}
{/snippet}

{#if flags.length === 0}
	<p class="card p-5 text-fg-muted">No flags yet. A flag is added in code.</p>
{:else}
	<ResponsiveTable rows={flags} key={(flag) => flag.key} from="xl">
		{#snippet head()}<th>Flag</th><th>On for</th><th>Changed</th>{/snippet}
		{#snippet row(flag)}
			<td>{@render who(flag)}</td>
			<td class="whitespace-nowrap">{@render onFor(flag)}</td>
			<td class="whitespace-nowrap text-fg-muted">{flag.updatedAt ? formatDate(flag.updatedAt) : 'Never'}</td>
		{/snippet}
		{#snippet card(flag)}
			<div>{@render who(flag)}</div>
			<p class="text-label text-fg-muted">{@render onFor(flag)}</p>
		{/snippet}
	</ResponsiveTable>
{/if}
