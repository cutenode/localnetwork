<script lang="ts">
	import { page } from '$app/state';
	import { Activity, ClipboardCheck, Disc3, LayoutDashboard, ScrollText, ShieldCheck } from '@lucide/svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const items = $derived([
		{ href: '/admin', label: 'Overview', icon: LayoutDashboard, exact: true },
		{ href: '/admin/requests', label: 'Requests', icon: ClipboardCheck, badge: data.pendingRequests },
		{ href: '/admin/projects', label: 'Projects', icon: Disc3 },
		{ href: '/admin/audit', label: 'Audit log', icon: ScrollText },
		...(data.viewer?.isAdmin
			? [
					{ href: '/admin/staff', label: 'Staff', icon: ShieldCheck },
					{ href: '/admin/system', label: 'System', icon: Activity }
				]
			: [])
	]);

	const isActive = (href: string, exact = false) => (exact ? page.url.pathname === href : page.url.pathname.startsWith(href));
</script>

<div class="container-page grid gap-8 py-8 lg:grid-cols-[13rem_1fr]">
	<nav class="flex gap-1 overflow-x-auto lg:flex-col" aria-label="Staff">
		<p class="eyebrow mb-2 hidden px-2 lg:block">{data.viewer?.isAdmin ? 'Admin' : 'Moderator'}</p>
		{#each items as item (item.href)}
			<a href={item.href} class="flex items-center gap-2 px-2 py-0.5 text-xl whitespace-nowrap {isActive(item.href, item.exact) ? 'bg-ansi-cyan text-ansi-black' : 'text-ansi-bright-cyan hover:bg-ansi-blue hover:text-ansi-white'}">
				<item.icon class="size-4" />{item.label}
				{#if item.badge}<span class="ml-auto bg-ansi-yellow px-1 text-base text-ansi-black">{item.badge}</span>{/if}
			</a>
		{/each}
	</nav>
	<div class="min-w-0">{@render children()}</div>
</div>
