<script lang="ts">
	import './layout.css';
	import { onMount } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { page } from '$app/state';
	import favicon from '#lib/assets/favicon.svg';
	import { clearSignedOutDrafts } from '#lib/form-draft.svelte.ts';
	import { type NavLink, siteNav } from '#lib/nav.ts';
	import SiteHeader from '#lib/ui/SiteHeader.svelte';
	import { hotkeys, hotkeyTarget, loadHotkeyPreference, setHotkeysEnabled } from '#lib/hotkeys.svelte.ts';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	onMount(() => {
		// A sign-out is a full page load, landing here whether or not the page it came from had hydrated.
		clearSignedOutDrafts();
		// Says the page is interactive, so the end-to-end tests can wait for it instead of guessing.
		document.documentElement.setAttribute('data-hydrated', '');
	});

	/** A key for aria-keyshortcuts, only while keyboard shortcuts are on. */
	const shortcut = (key: string) => (hotkeys.enabled ? key : undefined);

	const nav = $derived(siteNav(data.viewer, data.pendingRequests));

	const withShortcut = (link: NavLink) => ({ ...link, shortcut: shortcut(link.key) });

	let menuOpen = $state(false);

	afterNavigate(() => {
		menuOpen = false;
	});

	// The header's keys, plus a few for pages it doesn't link to. Signed out, the dashboard's go through sign-in.
	const shortcuts = $derived<Record<string, string>>({
		h: '/',
		a: '/about',
		p: '/privacy',
		m: '/match',
		d: '/for-djs',
		b: '/dashboard/bookings',
		n: '/dashboard/projects/new',
		c: '/projects?unclaimed=on',
		...Object.fromEntries([...nav.links, ...nav.actions, ...(nav.account?.links ?? [])].map((link) => [link.key.toLowerCase(), link.href]))
	});

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
	<meta name="theme-color" content="#09090b" />
</svelte:head>

<svelte:window {onkeydown} />

<a href="#main" class="sr-only rounded-md focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:bg-fg focus:px-3 focus:py-2 focus:text-fg-inverse">Skip to content</a>

<SiteHeader
	links={nav.links.map(withShortcut)}
	actions={nav.actions.map(withShortcut)}
	account={nav.account && { ...nav.account, links: nav.account.links.map(withShortcut) }}
	current={page.url.pathname}
	bind:open={menuOpen}
/>

<main id="main" class="min-h-[60vh]">
	{@render children()}
</main>

<footer class="border-t border-line print:hidden">
	<div class="container-page flex flex-wrap items-center gap-x-6 gap-y-3 py-8 text-small">
		<a class="menu-link" href="/about" aria-keyshortcuts={shortcut('A')}>How it works</a>
		<a class="menu-link" href="/privacy" aria-keyshortcuts={shortcut('P')}>Privacy</a>
		<a class="menu-link" href="/tools">Tools</a>
		<a class="menu-link" href="/report?from={encodeURIComponent(page.url.pathname)}">Report a bug</a>
		<button type="button" class="menu-link inline-flex min-h-9 items-center gap-2" aria-pressed={hotkeys.enabled} onclick={() => setHotkeysEnabled(!hotkeys.enabled)}>Keyboard shortcuts: {hotkeys.enabled ? 'on' : 'off'}</button>
		<span class="text-fg-subtle sm:ml-auto">Records on the AT Protocol · private data in atproto spaces</span>
	</div>
</footer>
