<script lang="ts">
	import { accountLabel, profileHref } from '#lib/format.ts';
	import type { AccountView } from '#lib/types.ts';

	/** An account by its handle, linking to its profile unless `link` is false (inside another link or a label). */
	let {
		account,
		link = true,
		class: className = ''
	}: { account: AccountView | null | undefined; link?: boolean; class?: string } = $props();
</script>

{#if account && link}
	<a href={profileHref(account)} class="text-fg underline decoration-fg-subtle underline-offset-4 hover:decoration-fg {className}" title={account.did}>{accountLabel(account)}</a>
{:else}
	<span class="text-fg {className}" title={account?.did}>{accountLabel(account)}</span>
{/if}
