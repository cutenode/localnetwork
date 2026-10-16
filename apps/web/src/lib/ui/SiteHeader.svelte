<script lang="ts">
	import { Menu, X } from '@lucide/svelte';
	import Button from './Button.svelte';

	/** `shortcut` is the key that opens it (announced, not drawn); `badge` a count waiting there. */
	type Link = { href: string; label: string; shortcut?: string; badge?: number };

	/**
	 * The bar across the top of every page. Wide, the links sit inline; narrower they fold into a
	 * menu, so the bar stays one row: the logo, the call to action, and the menu button. `account`
	 * (sign in, or your account) moves into the menu on a phone. `current` is the page's path; a
	 * link is current on its own page and the pages under it.
	 */
	let { links, account, cta, current, open = $bindable(false) }: { links: Link[]; account: Link; cta: Link; current?: string; open?: boolean } = $props();

	const isCurrent = (href: string) => current !== undefined && (current === href || current.startsWith(`${href}/`));
</script>

{#snippet item(link: Link, classes: string)}
	<a href={link.href} aria-current={isCurrent(link.href) ? 'page' : undefined} aria-keyshortcuts={link.shortcut} class={classes}
		>{link.label}{#if link.badge}<span class="pill bg-warning text-fg-inverse" title="Waiting for review">{link.badge}</span>{/if}</a
	>
{/snippet}

<header class="border-b border-line bg-canvas print:hidden">
	<div class="container-page flex min-h-16 items-center gap-x-8">
		<a href="/" class="flex items-center gap-2.5 text-emphasis font-semibold text-fg no-underline">
			<span aria-hidden="true" class="flex h-5 items-center gap-0.5">
				{#each [['h-2', 'bg-band-low'], ['h-4.5', 'bg-band-low'], ['h-3', 'bg-band-mid'], ['h-5', 'bg-band-high'], ['h-2.5', 'bg-band-low']] as [height, colour], i (i)}<span
						class="w-0.75 rounded-[1px] {height} {colour}"
					></span>{/each}
			</span>
			Local Network
		</a>
		<nav aria-label="Main" class="hidden flex-auto gap-x-6.5 text-small lg:flex">
			{#each links as link (link.href)}{@render item(link, 'menu-link inline-flex items-center gap-1.5')}{/each}
		</nav>
		<div class="ml-auto flex items-center gap-2">
			<Button href={account.href} variant="ghost" size="sm" class="max-lg:hidden" aria-current={isCurrent(account.href) ? 'page' : undefined} aria-keyshortcuts={account.shortcut}>{account.label}</Button>
			<Button href={cta.href} size="sm">{cta.label}</Button>
			<button
				type="button"
				aria-expanded={open}
				aria-controls="site-menu"
				aria-label={open ? 'Close menu' : 'Menu'}
				onclick={() => (open = !open)}
				class="-mr-2 grid size-11 place-items-center rounded-md text-fg hover:bg-raised lg:hidden"
			>
				{#if open}<X size={20} />{:else}<Menu size={20} />{/if}
			</button>
		</div>
	</div>
	{#if open}
		<nav id="site-menu" aria-label="Menu" class="container-page flex flex-col border-t border-line pt-2 pb-4 lg:hidden">
			{#each [...links, account] as link (link.href)}
				{@render item(link, 'flex min-h-12 items-center gap-2 border-b border-line text-body font-medium text-fg no-underline last:border-b-0')}
			{/each}
		</nav>
	{/if}
</header>
