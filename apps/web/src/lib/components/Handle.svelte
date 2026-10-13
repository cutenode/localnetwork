<script lang="ts">
	import { accountLabel } from '#lib/format.ts';
	import type { AccountView } from '#lib/types.ts';

	let { account, class: className = '' }: { account: AccountView | null | undefined; class?: string } = $props();
</script>

<span class="text-fg {className}" title={account?.did}>{accountLabel(account)}</span>
