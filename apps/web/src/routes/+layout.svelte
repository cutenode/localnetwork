<script lang="ts">
	import './layout.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import favicon from '#lib/assets/favicon.svg';
	import Hotkey from '#lib/components/Hotkey.svelte';
	import { accountLabel } from '#lib/format.ts';
	import { hotkeys, hotkeyTarget, loadHotkeyPreference, setHotkeysEnabled } from '#lib/hotkeys.svelte.ts';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	type MenuItem = { key: string; href: string; label: string; badge?: number };

	const menu = $derived<MenuItem[]>([
		{ key: 'F', href: '/projects', label: 'Find DJs' },
		{ key: 'M', href: '/match', label: 'Match a show' },
		{ key: 'D', href: '/for-djs', label: 'For DJs' },
		{ key: 'O', href: '/tools', label: 'Tools' },
		...(data.viewer
			? [
					{ key: 'B', href: '/bookings', label: 'Bookings' },
					{ key: 'U', href: '/me', label: 'Your account' },
					...(data.viewer.isStaff ? [{ key: 'T', href: '/admin', label: 'Staff', badge: data.pendingRequests }] : [])
				]
			: [{ key: 'S', href: '/login', label: 'Sign in' }])
	]);

	const shortcuts = $derived<Record<string, string>>({
		h: '/',
		a: '/about',
		p: '/privacy',
		n: '/projects/new',
		c: '/projects?unclaimed=on',
		...Object.fromEntries(menu.map((item) => [item.key.toLowerCase(), item.href]))
	});

	const logoColors = ['text-ansi-bright-cyan', 'text-ansi-bright-cyan', 'text-ansi-bright-magenta', 'text-ansi-bright-magenta', 'text-ansi-yellow', 'text-ansi-yellow'];

	let colorIndex = 0;

	const logo = ['LOCAL', 'NETWORK'].map((word) => [...word].map((ch) => ({ ch, color: logoColors[colorIndex++ % logoColors.length] })));

	const home = $derived(page.url.pathname === '/');

	const active = (href: string) => page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);

	$effect(() => {
		loadHotkeyPreference();
	});

	function onkeydown(event: KeyboardEvent) {
		const href = hotkeyTarget(event, shortcuts);

		if (!href) return;
		event.preventDefault();
		void goto(href);
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<meta name="theme-color" content="#0000aa" />
</svelte:head>

<svelte:window {onkeydown} />

<a href="#main" class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:bg-ansi-yellow focus:px-3 focus:py-1 focus:text-ansi-black">Skip to content</a>

<div class="bg-ansi-blue text-ansi-yellow">
	<div class="container-page flex flex-wrap items-center justify-between gap-x-6">
		<span>LOCAL NETWORK BBS</span>
		{#if data.viewer}
			<span class="flex items-center gap-3">
				<span class="text-ansi-white">{accountLabel(data.viewer)}</span>
				<form method="POST" action="/logout"><button class="px-1 hover:bg-ansi-yellow hover:text-ansi-black">Sign out</button></form>
			</span>
		{:else}
			<span>Guest</span>
		{/if}
	</div>
</div>

<header class="container-page flex flex-col gap-5 pt-6 sm:pt-8">
	<a href="/" aria-label="Local Network home" class="ansi-logo self-start {home ? 'text-[clamp(3.5rem,11vw,9.5rem)]' : 'text-5xl sm:text-6xl'}">
		{#each logo as word, w (w)}{#if w > 0}{' '}{/if}<span class="whitespace-nowrap">{#each word as letter, i (i)}<span class={letter.color}>{letter.ch}</span>{/each}</span>{/each}
	</a>
	<nav aria-label="Main" class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,12rem),1fr))] gap-x-6 border-[6px] border-double border-ansi-cyan px-4 py-1.5 text-xl sm:px-5">
		{#each menu as item (item.href)}
			<a href={item.href} class="menu-link flex items-center gap-2 px-1" aria-current={active(item.href) ? 'page' : undefined} aria-keyshortcuts={hotkeys.enabled ? item.key : undefined}>
				<Hotkey key={item.key} />{item.label}
				{#if item.badge}<span class="bg-ansi-yellow px-1 text-base text-ansi-black" title="Requests waiting for review">{item.badge}</span>{/if}
			</a>
		{/each}
	</nav>
</header>

<main id="main" class="min-h-[60vh]">
	{@render children()}
</main>

<footer class="mt-20 bg-ansi-blue text-ansi-white">
	<div class="container-page flex flex-wrap items-center gap-x-6 gap-y-1 py-0.5">
		<a class="menu-link flex items-center gap-1 px-1 text-ansi-white" href="/about" aria-keyshortcuts={hotkeys.enabled ? 'A' : undefined}><Hotkey key="A" />How it works</a>
		<a class="menu-link flex items-center gap-1 px-1 text-ansi-white" href="/privacy" aria-keyshortcuts={hotkeys.enabled ? 'P' : undefined}><Hotkey key="P" />Privacy</a>
		<button type="button" class="px-1 text-ansi-bright-cyan hover:bg-ansi-cyan hover:text-ansi-black" aria-pressed={hotkeys.enabled} onclick={() => setHotkeysEnabled(!hotkeys.enabled)}>Hotkeys: {hotkeys.enabled ? 'on' : 'off'}</button>
		<span class="text-ansi-bright-cyan sm:ml-auto">Records on the AT Protocol · private data in atproto spaces</span>
	</div>
</footer>
