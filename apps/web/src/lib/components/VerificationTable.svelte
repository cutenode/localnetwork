<script lang="ts">
	import { enhance } from '$app/forms';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { IssuedVerification, LiveIdentity } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';
	import { isCurrent } from '#lib/verification.ts';

	let {
		verifications,
		live
	}: {
		/** Null when they couldn't be read. */
		verifications: IssuedVerification[] | null;
		/** Each subject's handle and display name now, by DID; a pending one shows a placeholder until it arrives. */
		live: Record<string, LiveIdentity | Promise<LiveIdentity>>;
	} = $props();
</script>

{#snippet account(verification: IssuedVerification)}
	<p>@{verification.handle}</p>
	<p class="font-mono text-label text-fg-subtle [overflow-wrap:anywhere]">{verification.subject}</p>
{/snippet}

{#snippet checking()}
	<span class="inline-block h-6 w-20 animate-pulse rounded-full bg-raised" aria-hidden="true"></span><span class="sr-only">Checking</span>
{/snippet}

{#snippet status(verification: IssuedVerification, now: LiveIdentity | undefined)}
	{@const current = !!now && isCurrent(verification, now)}
	<StatusPill status={current ? 'verified' : 'outdated'} />
	{#if now && !current}
		<p class="mt-1 text-label text-fg-muted">Now {now.handle ? `@${now.handle}` : 'no working handle'} · {now.displayName || 'no display name'}</p>
	{/if}
{/snippet}

{#snippet actions(verification: IssuedVerification, now: LiveIdentity | undefined)}
	{#if now?.handle && now.displayName && !isCurrent(verification, now)}
		<form method="POST" action="?/grant" use:enhance>
			<input type="hidden" name="subject" value={verification.subject} />
			<button class="btn-secondary btn-sm">Re-issue</button>
		</form>
	{/if}
	<form method="POST" action="?/revoke" use:enhance={({ cancel }) => { if (!confirm('Remove this verification?')) cancel(); }}>
		<input type="hidden" name="did" value={verification.subject} />
		<button class="btn-danger btn-sm">Remove</button>
	</form>
{/snippet}

{#if !verifications}
	<div class="card p-5 text-small text-fg-muted">Couldn't load verifications. <a class="link" href="/admin/verifications">Try again</a></div>
{:else if verifications.length}
	<ResponsiveTable rows={verifications} key={(verification) => verification.uri} from="xl">
		{#snippet head()}<th>Account</th><th class="whitespace-nowrap">Verified as</th><th>Status</th><th><span class="sr-only">Actions</span></th>{/snippet}
		{#snippet row(verification)}
			<td>{@render account(verification)}</td>
			<td>
				<p>{verification.displayName}</p>
				<p class="whitespace-nowrap text-label text-fg-muted">{formatDate(verification.createdAt)}</p>
			</td>
			{#await live[verification.subject]}
				<td>{@render checking()}</td>
				<td></td>
			{:then now}
				<td>{@render status(verification, now)}</td>
				<td><div class="flex justify-end gap-2">{@render actions(verification, now)}</div></td>
			{/await}
		{/snippet}
		{#snippet card(verification)}
			<div class="flex flex-col gap-1">
				{@render account(verification)}
				<p class="text-label text-fg-muted">Verified as {verification.displayName} on {formatDate(verification.createdAt)}</p>
			</div>
			{#await live[verification.subject]}
				<div>{@render checking()}</div>
			{:then now}
				<div>{@render status(verification, now)}</div>
				<div class="flex flex-wrap gap-2">{@render actions(verification, now)}</div>
			{/await}
		{/snippet}
	</ResponsiveTable>
{:else}
	<div class="card p-5 text-small text-fg-muted">No one is verified yet.</div>
{/if}
