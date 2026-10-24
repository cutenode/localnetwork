<script lang="ts">
	import { page } from '$app/state';
	import { Activity, BadgeCheck, ClipboardCheck, Disc3, LayoutDashboard, ScrollText, ShieldCheck } from '@lucide/svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
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
					{ href: '/admin/verifications', label: 'Verifications', icon: BadgeCheck },
					{ href: '/admin/system', label: 'System', icon: Activity }
				]
			: [])
	]);

	const isActive = (href: string, exact = false) => (exact ? page.url.pathname === href : page.url.pathname.startsWith(href));
</script>

<div class="container-page grid grid-cols-[minmax(0,1fr)] gap-8 py-6 sm:py-10 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-10">
	<!-- A sidebar on wide screens. Narrower, a row under the header that scrolls sideways. -->
	<nav aria-label="Staff" class="max-lg:border-b max-lg:border-line max-lg:pb-3">
		<div class="mb-3 hidden px-3 lg:block"><Eyebrow>{data.viewer?.isAdmin ? 'Admin' : 'Moderator'}</Eyebrow></div>
		<ul class="flex gap-1 overflow-x-auto max-sm:-mx-4 max-sm:px-4 lg:flex-col">
			{#each items as item (item.href)}
				{@const active = isActive(item.href, item.exact)}
				<li class="shrink-0">
					<!-- In the sideways row a later section can start out of view, so bring the current one in. -->
					<a href={item.href} aria-current={active ? 'page' : undefined} {@attach (link) => { if (active) link.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }} class={['flex min-h-9 items-center gap-2.5 rounded-md px-3 text-small whitespace-nowrap no-underline transition-colors', active ? 'bg-raised font-medium text-fg' : 'text-fg-muted hover:bg-raised hover:text-fg']}>
						<item.icon class="size-4 shrink-0" aria-hidden="true" />{item.label}
						{#if item.badge}<span class="ml-auto rounded-full bg-fg px-1.5 font-mono text-caption font-semibold text-fg-inverse">{item.badge}</span>{/if}
					</a>
				</li>
			{/each}
		</ul>
	</nav>
	<div class="min-w-0">{@render children()}</div>
</div>
