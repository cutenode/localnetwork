<script lang="ts">
	import './layout.css';
	import { onMount } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { page } from '$app/state';
	import favicon from '#lib/assets/favicon.svg';
	import { accountLabel } from '#lib/format.ts';
	import SiteHeader from '#lib/ui/SiteHeader.svelte';
	import { hotkeys, hotkeyTarget, loadHotkeyPreference, setHotkeysEnabled } from '#lib/hotkeys.svelte.ts';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	// Says the page is interactive, so the end-to-end tests can wait for it instead of guessing.
	onMount(() => document.documentElement.setAttribute('data-hydrated', ''));

	/** A key for aria-keyshortcuts, only while keyboard shortcuts are on. */
	const shortcut = (key: string) => (hotkeys.enabled ? key : undefined);

	type MenuItem = { key: string; href: string; label: string; badge?: number };

	const items = $derived<MenuItem[]>([
		{ key: 'F', href: '/projects', label: 'Find DJs' },
		{ key: 'W', href: '/shows', label: 'Shows' },
		{ key: 'M', href: '/match', label: 'Match a show' },
		{ key: 'R', href: '/charts', label: 'Charts' },
		{ key: 'D', href: '/for-djs', label: 'For DJs' },
		{ key: 'O', href: '/tools', label: 'Tools' },
		...(data.viewer
			? [{ key: 'B', href: '/dashboard/bookings', label: 'Bookings' }, ...(data.viewer.isStaff ? [{ key: 'T', href: '/admin', label: 'Staff', badge: data.pendingRequests }] : [])]
			: [])
	]);

	const menu = $derived(items.map((item) => ({ ...item, shortcut: shortcut(item.key) })));

	const account = $derived(data.viewer ? { href: '/dashboard', label: accountLabel(data.viewer), shortcut: shortcut('U') } : { href: '/login', label: 'Sign in', shortcut: shortcut('S') });

	let menuOpen = $state(false);

	afterNavigate(() => {
		menuOpen = false;
	});

	const shortcuts = $derived<Record<string, string>>({
		h: '/',
		a: '/about',
		p: '/privacy',
		n: '/dashboard/projects/new',
		c: '/projects?unclaimed=on',
		[data.viewer ? 'u' : 's']: account.href,
		...Object.fromEntries(items.map((item) => [item.key.toLowerCase(), item.href]))
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

<SiteHeader links={menu} {account} cta={{ href: '/dashboard/projects/new', label: 'List your project', shortcut: shortcut('N') }} current={page.url.pathname} bind:open={menuOpen} />

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
