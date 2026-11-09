<script lang="ts">
	import { page } from '$app/state';
	import { CalendarPlus, ClipboardList, Database, History, Inbox, House, NotebookPen, Plus, ShieldCheck } from '@lucide/svelte';
	import { accountLabel } from '#lib/format.ts';
	import SidebarLayout from '#lib/ui/SidebarLayout.svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const main = $derived([
		{ href: '/dashboard', label: 'Home', icon: House, exact: true },
		{ href: '/dashboard/inbox', label: 'Inbox', icon: Inbox, dot: data.unread > 0 },
		{ href: '/dashboard/bookings', label: 'Bookings', icon: CalendarPlus }
	]);

	// The profile's sections decide what's here: a DJ's projects, and a promoter's planner. Until there's a profile, or
	// while the directory can't say, everything shows; projects someone manages always do.
	const groups = $derived([
		{ items: main },
		{
			continues: true,
			items: data.sections.then((sections) => (!sections || sections.promoter ? [{ href: '/dashboard/planner', label: 'Planner', icon: NotebookPen }] : []))
		},
		{
			label: 'Yours',
			items: Promise.all([data.projects, data.sections]).then(([projects, sections]) => [
				{ href: '/dashboard/profile', label: 'You', monogram: accountLabel(data.viewer) },
				...(projects ?? []).map((project) => ({ href: `/dashboard/projects/${project.slug}`, label: project.profile.name, monogram: project.profile.name })),
				...(!sections || sections.performer || projects?.length ? [{ href: '/dashboard/projects/new', label: 'List a project', icon: Plus, exact: true }] : [])
			])
		}
	]);

	const footer = $derived([
		{ href: '/dashboard/requests', label: 'Submissions', icon: ClipboardList },
		{ href: '/dashboard/activity', label: 'Activity', icon: History },
		{ href: '/dashboard/data', label: 'Your data', icon: Database },
		...(data.viewer?.isStaff ? [{ href: '/admin', label: 'Staff console', icon: ShieldCheck, badge: data.pendingRequests }] : [])
	]);
</script>

<SidebarLayout label="Dashboard" {groups} {footer} tabs={main} current={page.url.pathname}>{@render children()}</SidebarLayout>
