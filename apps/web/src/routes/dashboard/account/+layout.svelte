<script lang="ts">
	import { page } from '$app/state';
	import AccountNav from '#lib/components/AccountNav.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<AccountNav path={page.url.pathname} />
{@render children()}
