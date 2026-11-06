<script lang="ts">
	import Handle from '#lib/components/Handle.svelte';
	import type { StrayMember } from '#lib/types.ts';

	/** Accounts left in private spaces with no role, or null when that couldn't be checked. */
	let { strays }: { strays: StrayMember[] | null } = $props();
</script>

{#if strays === null}
	<p role="alert" class="notice notice-error m-0">Couldn't check the private spaces. Reload the page to try again.</p>
{:else if strays.length > 0}
	<section class="card flex flex-col gap-3 p-5" aria-labelledby="stray-members">
		<div>
			<h2 id="stray-members" class="text-emphasis font-semibold">Still in private spaces</h2>
			<p class="text-small text-fg-muted">These accounts have no role, but their removal hasn't gone through. It's retried every few minutes.</p>
		</div>
		<ul class="flex flex-col gap-2">
			{#each strays as stray (stray.did)}
				<li>
					<Handle account={stray} />
					<p class="text-label text-fg-muted capitalize">{stray.spaces.join(', ')}</p>
				</li>
			{/each}
		</ul>
	</section>
{/if}
