<script lang="ts">
	import { Menu, X } from '@lucide/svelte';
	import Monogram from './Monogram.svelte';

	/**
	 * `shortcut` is the key that opens it (announced, not drawn); `badge` a count waiting there; `also`
	 * other paths that belong to it, so it stays current on them.
	 */
	type Link = { href: string; label: string; shortcut?: string; badge?: number; also?: string[] };

	/**
	 * Who is signed in: `handle` heads the menu and `name` draws the monogram. `alert` (say, "3 requests
	 * waiting") puts a dot on the button and is read out with it. `signOut` is the form action.
	 */
	type Account = { handle: string; name: string; links: Link[]; alert?: string; signOut: string };

	/**
	 * The bar across the top of every page: the logo and `links` on the left, `actions` on the right
	 * (the last one drawn as a button), then the account menu when someone is signed in. On a phone it
	 * all folds into one menu. `current` is the page's path; a link is current on its own page, the
	 * pages under it, and its `also` paths.
	 */
	let { links, actions, account, current, open = $bindable(false) }: { links: Link[]; actions: Link[]; account?: Account; current?: string; open?: boolean } = $props();

	const id = $props.id();

	let accountOpen = $state(false);

	let accountMenu = $state<HTMLElement>();

	let accountButton = $state<HTMLButtonElement>();

	let menuButton = $state<HTMLButtonElement>();

	const isCurrent = (link: Link) => current !== undefined && [link.href, ...(link.also ?? [])].some((href) => current === href || current.startsWith(`${href}/`));

	const ariaCurrent = (link: Link) => (isCurrent(link) ? 'page' : undefined);

	/** Escape closes whichever menu is open and puts focus back on its button. */
	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape') return;

		if (accountOpen) {
			accountOpen = false;
			accountButton?.focus();
		} else if (open) {
			open = false;
			menuButton?.focus();
		}
	}

	/** A click or tap anywhere outside the account menu closes it. */
	function onpointerdown(event: PointerEvent) {
		if (accountOpen && !(event.target instanceof Node && accountMenu?.contains(event.target))) accountOpen = false;
	}

	/** Tabbing out of the account menu closes it. */
	function onfocusout(event: FocusEvent) {
		const next = event.relatedTarget;

		if (next instanceof Node && !accountMenu?.contains(next)) accountOpen = false;
	}
</script>

<svelte:window {onkeydown} {onpointerdown} />

{#snippet badge(count: number | undefined)}
	{#if count}<span class="ml-auto rounded-full bg-fg px-1.5 font-mono text-caption font-semibold text-fg-inverse">{count}</span>{/if}
{/snippet}

{#snippet dot()}
	<span aria-hidden="true" class="absolute top-1.5 right-1.5 size-2.5 rounded-full bg-warning ring-2 ring-canvas"></span>
{/snippet}

<!-- The account's own items, in the menu on a wide screen and at the end of the phone menu. -->
{#snippet accountItems(signedIn: Account, itemClass: string)}
	{#each signedIn.links as link (link.href)}
		<a href={link.href} aria-current={ariaCurrent(link)} aria-keyshortcuts={link.shortcut} onclick={() => (accountOpen = false)} class={itemClass}>{link.label} {@render badge(link.badge)}</a>
	{/each}
	<form method="POST" action={signedIn.signOut} class="flex">
		<button class={[itemClass, 'w-full text-left']}>Sign out</button>
	</form>
{/snippet}

<header class="border-b border-line bg-canvas print:hidden">
	<div class="container-page flex min-h-16 items-center gap-x-8">
		<a href="/" class="flex shrink-0 items-center gap-2.5 text-emphasis font-semibold whitespace-nowrap text-fg no-underline">
			<span aria-hidden="true" class="flex h-5 items-center gap-0.5">
				{#each [['h-2', 'bg-band-low'], ['h-4.5', 'bg-band-low'], ['h-3', 'bg-band-mid'], ['h-5', 'bg-band-high'], ['h-2.5', 'bg-band-low']] as [height, colour], i (i)}<span
						class="w-0.75 rounded-[1px] {height} {colour}"
					></span>{/each}
			</span>
			Local Network
		</a>
		<nav aria-label="Main" class="hidden flex-auto gap-x-6.5 text-small md:flex">
			{#each links as link (link.href)}<a href={link.href} aria-current={ariaCurrent(link)} aria-keyshortcuts={link.shortcut} class="menu-link">{link.label}</a>{/each}
		</nav>
		<div class="ml-auto flex items-center gap-x-5">
			{#each actions as link, index (link.href)}
				<a href={link.href} aria-current={ariaCurrent(link)} aria-keyshortcuts={link.shortcut} class={['max-md:hidden', index === actions.length - 1 ? 'btn-secondary btn-sm aria-[current=page]:border-fg-subtle' : 'menu-link text-small']}>{link.label}</a>
			{/each}
			{#if account}
				<div bind:this={accountMenu} {onfocusout} class="relative -mr-1.5 max-md:hidden">
					<button
						bind:this={accountButton}
						type="button"
						aria-expanded={accountOpen}
						aria-controls="{id}-account"
						onclick={() => (accountOpen = !accountOpen)}
						class="relative grid size-11 place-items-center rounded-lg hover:bg-raised"
					>
						<!-- Named in words, since the monogram's letters alone don't say what it opens. -->
						<span class="sr-only">{['Account', account.handle, account.alert].filter(Boolean).join(', ')}</span>
						<Monogram name={account.name} size="sm" />
						{#if account.alert}{@render dot()}{/if}
					</button>
					{#if accountOpen}
						<div id="{id}-account" class="absolute top-full right-0 z-30 mt-1.5 flex w-60 flex-col gap-0.5 rounded-xl border border-line-strong bg-raised p-1.5">
							<p class="m-0 truncate border-b border-line px-3 pt-1.5 pb-2.5 mb-1 text-small font-medium text-fg">{account.handle}</p>
							{@render accountItems(account, 'flex min-h-10 items-center gap-2.5 rounded-md px-3 text-small text-fg-muted no-underline transition-colors hover:bg-well hover:text-fg aria-[current=page]:text-fg')}
						</div>
					{/if}
				</div>
			{/if}
			<button
				bind:this={menuButton}
				type="button"
				aria-expanded={open}
				aria-controls="{id}-menu"
				aria-label={open ? 'Close menu' : ['Menu', account?.alert].filter(Boolean).join(', ')}
				onclick={() => (open = !open)}
				class="relative -mr-2 grid size-11 place-items-center rounded-md text-fg hover:bg-raised md:hidden"
			>
				{#if open}<X size={20} />{:else}<Menu size={20} />{/if}
				{#if account?.alert && !open}{@render dot()}{/if}
			</button>
		</div>
	</div>
	{#if open}
		{@const rowClass = 'flex min-h-12 w-full items-center gap-2 text-body font-medium text-fg no-underline'}
		<nav id="{id}-menu" aria-label="Menu" class="container-page flex flex-col border-t border-line pt-2 pb-4 md:hidden">
			<div class="flex flex-col divide-y divide-line">
				{#each links as link (link.href)}<a href={link.href} aria-current={ariaCurrent(link)} aria-keyshortcuts={link.shortcut} class={rowClass}>{link.label}</a>{/each}
			</div>
			<div class="mt-2 flex flex-col divide-y divide-line border-t border-line-strong pt-2">
				{#each actions as link (link.href)}<a href={link.href} aria-current={ariaCurrent(link)} aria-keyshortcuts={link.shortcut} class={rowClass}>{link.label}</a>{/each}
			</div>
			{#if account}
				<div class="mt-2 flex flex-col divide-y divide-line border-t border-line-strong pt-4">
					<p class="m-0 truncate pb-3 text-small text-fg-muted">{account.handle}</p>
					{@render accountItems(account, rowClass)}
				</div>
			{/if}
		</nav>
	{/if}
</header>
