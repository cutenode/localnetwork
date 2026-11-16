<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { ACCOUNT_PAGES } from '#lib/account.ts';

	/**
	 * The way around the Account area on `path`. A phone gets one step back, since the row of pages wouldn't fit; wider,
	 * they all show. The landing page is the list of these pages, so it needs neither.
	 */
	let { path }: { path: string } = $props();

	const section = $derived(ACCOUNT_PAGES.find(({ href }) => path === href || path.startsWith(`${href}/`)));

	const up = $derived(section && path !== section.href ? section : { href: '/dashboard/account', label: 'Account' });
</script>

{#if section}
	<div class="mb-6 max-w-3xl">
		<a href={up.href} class="inline-flex min-h-11 items-center gap-1.5 px-1 text-small text-fg-muted no-underline hover:text-fg sm:hidden"><ArrowLeft class="size-4" aria-hidden="true" />{up.label}</a>
		<nav aria-label="Account pages" class="flex gap-x-6 border-b border-line max-sm:hidden">
			<a href="/dashboard/account" class="tab">Account</a>
			{#each ACCOUNT_PAGES as { href, label } (href)}<a {href} aria-current={section.href === href ? 'page' : undefined} class="tab">{label}</a>{/each}
		</nav>
	</div>
{/if}

<style>
	.tab {
		display: inline-flex;
		align-items: center;
		min-height: 2.75rem;
		margin-bottom: -1px;
		border-bottom: 2px solid transparent;
		color: var(--color-fg-muted);
		font-size: var(--text-small);
		text-decoration: none;
		white-space: nowrap;
	}

	.tab:hover {
		color: var(--color-fg);
	}

	.tab[aria-current='page'] {
		border-bottom-color: var(--color-fg);
		color: var(--color-fg);
	}
</style>
