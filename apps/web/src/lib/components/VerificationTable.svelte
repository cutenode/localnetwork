<script lang="ts">
	import { enhance } from '$app/forms';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { IssuedVerification, LiveIdentity } from '#lib/types.ts';
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

{#if !verifications}
	<div class="card p-5 text-small text-fg-muted">Couldn't load verifications. <a class="link" href="/admin/verifications">Try again</a></div>
{:else if verifications.length}
	<!-- Relative, so the visually hidden header stays inside the scrolling box. -->
	<div class="card relative overflow-x-auto">
		<table class="table-base min-w-150">
			<thead><tr><th>Account</th><th class="whitespace-nowrap">Verified as</th><th>Status</th><th><span class="sr-only">Actions</span></th></tr></thead>
			<tbody>
				{#each verifications as verification (verification.uri)}
					<tr>
						<td>
							<p>@{verification.handle}</p>
							<p class="mono text-fg-subtle">{verification.subject}</p>
						</td>
						<td>
							<p>{verification.displayName}</p>
							<p class="whitespace-nowrap text-label text-fg-muted">{formatDate(verification.createdAt)}</p>
						</td>
						{#await live[verification.subject]}
							<td><span class="inline-block h-6 w-20 animate-pulse rounded-full bg-raised" aria-hidden="true"></span><span class="sr-only">Checking</span></td>
							<td></td>
						{:then now}
							{@const current = !!now && isCurrent(verification, now)}
							<td>
								<StatusPill status={current ? 'verified' : 'outdated'} />
								{#if now && !current}
									<p class="mt-1 text-label text-fg-muted">
										Now {now.handle ? `@${now.handle}` : 'no working handle'} · {now.displayName || 'no display name'}
									</p>
								{/if}
							</td>
							<td>
								<div class="flex justify-end gap-2">
									{#if now?.handle && now.displayName && !current}
										<form method="POST" action="?/grant" use:enhance>
											<input type="hidden" name="subject" value={verification.subject} />
											<button class="btn-secondary btn-sm">Re-issue</button>
										</form>
									{/if}
									<form method="POST" action="?/revoke" use:enhance={({ cancel }) => { if (!confirm('Remove this verification?')) cancel(); }}>
										<input type="hidden" name="did" value={verification.subject} />
										<button class="btn-danger btn-sm">Remove</button>
									</form>
								</div>
							</td>
						{/await}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{:else}
	<div class="card p-5 text-small text-fg-muted">No one is verified yet.</div>
{/if}
