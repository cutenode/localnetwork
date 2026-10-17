<script lang="ts">
	import { UserCheck, UserPlus } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import type { FollowState } from '#lib/types.ts';

	/**
	 * Follow or unfollow a project, with its follower count. `follow` is undefined while it loads and null when it
	 * can't; signed out (`loginHref`), the button links to sign-in. It keeps one width through every state, so the
	 * buttons beside it stay put.
	 */
	let { follow, loginHref }: { follow: FollowState | null | undefined; loginHref?: string } = $props();

	let sending = $state(false);

	const following = $derived(follow?.following ?? false);
</script>

{#snippet content()}
	{#if following}<UserCheck class="size-4" />Following{:else}<UserPlus class="size-4" />Follow{/if}
	<span class="min-w-[3ch] text-left font-mono text-fg-muted tabular-nums">
		{#if follow}{follow.followers.toLocaleString('en')}{' '}<span class="sr-only">{follow.followers === 1 ? 'follower' : 'followers'}</span>{:else if follow === undefined}<span class="inline-block h-3 w-[2ch] animate-pulse rounded-sm bg-raised align-middle"></span>{/if}
	</span>
{/snippet}

{#if loginHref}
	<a href={loginHref} class="btn-secondary min-w-36">{@render content()}</a>
{:else}
	<form
		method="POST"
		action={following ? '?/unfollow' : '?/follow'}
		use:enhance={() => {
			sending = true;

			return async ({ update }) => {
				await update({ reset: false });
				sending = false;
			};
		}}
	>
		<button class="btn-secondary min-w-36" aria-pressed={follow ? following : undefined} disabled={!follow || sending}>{@render content()}</button>
	</form>
{/if}
