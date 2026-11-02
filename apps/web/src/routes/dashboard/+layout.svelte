<script lang="ts">
	import { page } from '$app/state';
	import { CalendarPlus, ClipboardList, Disc3, Globe, History, Inbox, LayoutDashboard, NotebookPen, Plus, ShieldCheck, UserRound } from '@lucide/svelte';
	import SidebarLayout from '#lib/ui/SidebarLayout.svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	// The profile's sections decide the groups: a DJ's projects, and a promoter's planner. Until there's a profile, or
	// while the directory can't say, every group shows; projects someone manages always do.
	const groups = $derived([
		{
			items: [
				{ href: '/dashboard', label: 'Overview', icon: LayoutDashboard, exact: true },
				{ href: '/dashboard/profile', label: 'Profile', icon: UserRound },
				{ href: '/dashboard/inbox', label: 'Inbox', icon: Inbox, badge: data.unread },
				{ href: '/dashboard/bookings', label: 'Bookings', icon: CalendarPlus },
				{ href: '/dashboard/requests', label: 'Requests', icon: ClipboardList },
				{ href: '/dashboard/activity', label: 'Activity', icon: History }
			]
		},
		{
			label: 'Projects',
			items: Promise.all([data.projects, data.sections]).then(([projects, sections]) =>
				!sections || sections.performer || projects?.length
					? [
							...(projects ?? []).map((project) => ({ href: `/dashboard/projects/${project.slug}`, label: project.profile.name, icon: Disc3 })),
							{ href: '/dashboard/projects/new', label: 'List a project', icon: Plus }
						]
					: []
			)
		},
		{
			label: 'Shows',
			items: data.sections.then((sections) => (!sections || sections.promoter ? [{ href: '/dashboard/planner', label: 'Planner', icon: NotebookPen }] : []))
		}
	]);

	const footer = $derived([
		...(data.viewer?.isStaff ? [{ href: '/admin', label: 'Staff console', icon: ShieldCheck, badge: data.pendingRequests }] : []),
		{ href: '/', label: 'View site', icon: Globe, exact: true }
	]);
</script>

<SidebarLayout label="Dashboard" title="Dashboard" {groups} {footer} current={page.url.pathname}>{@render children()}</SidebarLayout>
