<script lang="ts">
	import { page } from '$app/state';
	import { CalendarPlus, ClipboardList, Database, History, Inbox, Headphones, House, NotebookPen, Plus, ShieldCheck } from '@lucide/svelte';
	import { accountLabel } from '#lib/format.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import SidebarLayout, { type SidebarItem } from '#lib/ui/SidebarLayout.svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const access = resolved(() => data.access, []);

	const main = $derived([
		{ href: '/dashboard', label: 'Home', icon: House, exact: true, badge: data.waiting + access.value.length },
		{ href: '/dashboard/inbox', label: 'Inbox', icon: Inbox, dot: data.unread > 0 },
		{ href: '/dashboard/bookings', label: 'Bookings', icon: CalendarPlus }
	]);

	const projects = resolved(() => data.projects, undefined);

	const sections = resolved(() => data.sections, undefined);

	const stillComing = new Promise<SidebarItem[]>(() => {});

	const planner = { href: '/dashboard/planner', label: 'Planner', icon: NotebookPen };

	// The profile's sections decide what's here: a DJ's projects, and a promoter's own section. Until there's a profile,
	// or while the directory can't say, the planner and List a project show anyway; projects someone manages always do.
	const unsure = $derived(sections.value ? [] : [planner]);

	const promoter = $derived(sections.value?.promoter ? [{ href: '/dashboard/promoter', label: 'Book DJs', icon: Headphones }, planner] : []);

	const yours = $derived([
		{ href: '/dashboard/profile', label: 'You', monogram: accountLabel(data.viewer) },
		...(projects.value ?? []).map((project) => ({ href: `/dashboard/projects/${project.slug}`, label: project.profile.name, monogram: project.profile.name })),
		...(!sections.value || sections.value.performer || projects.value?.length ? [{ href: '/dashboard/projects/new', label: 'List a project', icon: Plus, exact: true }] : [])
	]);

	const isKnown = $derived(projects.value !== undefined && sections.value !== undefined);

	// The Promoter section shows no placeholders while it's looked up, since most people won't have one.
	const groups = $derived([
		{ items: main },
		{ continues: true, items: isKnown ? unsure : stillComing },
		{ label: 'Promoter', continues: !isKnown, items: isKnown ? promoter : stillComing },
		{ label: 'Yours', items: isKnown ? yours : stillComing }
	]);

	const footer = $derived([
		{ href: '/dashboard/requests', label: 'Submissions', icon: ClipboardList },
		{ href: '/dashboard/activity', label: 'Activity', icon: History },
		{ href: '/dashboard/data', label: 'Your data', icon: Database },
		...(data.viewer?.isStaff ? [{ href: '/admin', label: 'Staff console', icon: ShieldCheck, badge: data.pendingRequests }] : [])
	]);
</script>

<SidebarLayout label="Dashboard" {groups} {footer} tabs={main} current={page.url.pathname}>{@render children()}</SidebarLayout>
