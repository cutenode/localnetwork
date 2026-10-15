<script lang="ts">
	import { Menu, X } from '@lucide/svelte';
	import Button from './Button.svelte';

	type Link = { href: string; label: string };

	/**
	 * The bar across the top of every page. Wide, the links sit inline; on a phone they fold into a
	 * menu, so the bar stays one row: the logo, the call to action, and the menu button. `account`
	 * (sign in, or your account) moves into the menu on a phone.
	 */
	let { links, account, cta, current, open = $bindable(false) }: { links: Link[]; account: Link; cta: Link; current?: string; open?: boolean } = $props();
</script>

<header class="border-b border-line bg-canvas">
	<div class="container-page flex min-h-16 items-center gap-x-8">
		<a href="/" class="flex items-center gap-2.5 text-emphasis font-semibold text-fg no-underline">
			<span aria-hidden="true" class="flex h-5 items-center gap-0.5">
				{#each [['h-2', 'bg-band-low'], ['h-4.5', 'bg-band-low'], ['h-3', 'bg-band-mid'], ['h-5', 'bg-band-high'], ['h-2.5', 'bg-band-low']] as [height, colour], i (i)}<span
						class="w-0.75 rounded-[1px] {height} {colour}"
					></span>{/each}
			</span>
			Local Network
		</a>
		<nav aria-label="Main" class="hidden flex-auto gap-x-6.5 text-small md:flex">
			{#each links as link (link.href)}<a href={link.href} aria-current={link.href === current ? 'page' : undefined} class="text-fg-muted no-underline hover:text-fg aria-[current=page]:text-fg">{link.label}</a>{/each}
		</nav>
		<div class="ml-auto flex items-center gap-2">
			<Button href={account.href} variant="ghost" size="sm" class="max-md:hidden">{account.label}</Button>
			<Button href={cta.href} size="sm">{cta.label}</Button>
			<button
				type="button"
				aria-expanded={open}
				aria-controls="site-menu"
				aria-label={open ? 'Close menu' : 'Menu'}
				onclick={() => (open = !open)}
				class="-mr-2 grid size-11 place-items-center rounded-md text-fg hover:bg-raised md:hidden"
			>
				{#if open}<X size={20} />{:else}<Menu size={20} />{/if}
			</button>
		</div>
	</div>
	{#if open}
		<nav id="site-menu" aria-label="Menu" class="container-page flex flex-col border-t border-line pt-2 pb-4 md:hidden">
			{#each [...links, account] as link (link.href)}
				<a href={link.href} aria-current={link.href === current ? 'page' : undefined} class="flex min-h-12 items-center border-b border-line text-body font-medium text-fg no-underline last:border-b-0">{link.label}</a>
			{/each}
		</nav>
	{/if}
</header>
