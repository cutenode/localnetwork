<script lang="ts">
	import { page } from '$app/state';
	import { CalendarPlus, ClipboardList, Disc3, Globe, History, LayoutDashboard, NotebookPen, Plus, ShieldCheck } from '@lucide/svelte';
	import SidebarLayout from '#lib/ui/SidebarLayout.svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const groups = $derived([
		{
			items: [
				{ href: '/dashboard', label: 'Overview', icon: LayoutDashboard, exact: true },
				{ href: '/dashboard/bookings', label: 'Bookings', icon: CalendarPlus },
				{ href: '/dashboard/planner', label: 'Planner', icon: NotebookPen },
				{ href: '/dashboard/requests', label: 'Requests', icon: ClipboardList },
				{ href: '/dashboard/activity', label: 'Activity', icon: History }
			]
		},
		{
			label: 'Projects',
			items: data.projects.then((projects) => [
				...(projects ?? []).map((project) => ({ href: `/dashboard/projects/${project.slug}`, label: project.profile.name, icon: Disc3 })),
				{ href: '/dashboard/projects/new', label: 'List a project', icon: Plus }
			])
		}
	]);

	const footer = $derived([
		...(data.viewer?.isStaff ? [{ href: '/admin', label: 'Staff console', icon: ShieldCheck, badge: data.pendingRequests }] : []),
		{ href: '/', label: 'View site', icon: Globe, exact: true }
	]);
</script>

<SidebarLayout label="Dashboard" title="Dashboard" {groups} {footer} current={page.url.pathname}>{@render children()}</SidebarLayout>
