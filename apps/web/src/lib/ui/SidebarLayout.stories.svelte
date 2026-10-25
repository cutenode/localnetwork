<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { Activity, CalendarPlus, ClipboardCheck, ClipboardList, Disc3, Globe, History, LayoutDashboard, NotebookPen, Plus, ScrollText, ShieldCheck } from '@lucide/svelte';
	import SidebarLayout, { type SidebarGroup } from './SidebarLayout.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Sidebar layout', component: SidebarLayout, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });

	const projects = [
		{ href: '/dashboard/projects/neon-tide', label: 'Neon Tide', icon: Disc3 },
		{ href: '/dashboard/projects/halflight', label: 'Halflight', icon: Disc3 }
	];

	const dashboard = (managed: SidebarGroup['items']): SidebarGroup[] => [
		{
			items: [
				{ href: '/dashboard', label: 'Overview', icon: LayoutDashboard, exact: true },
				{ href: '/dashboard/bookings', label: 'Bookings', icon: CalendarPlus },
				{ href: '/dashboard/planner', label: 'Planner', icon: NotebookPen },
				{ href: '/dashboard/requests', label: 'Requests', icon: ClipboardList },
				{ href: '/dashboard/activity', label: 'Activity', icon: History }
			]
		},
		{ label: 'Projects', items: managed }
	];

	const listed = [...projects, { href: '/dashboard/projects/new', label: 'List a project', icon: Plus }];

	const footer = [
		{ href: '/admin', label: 'Staff console', icon: ShieldCheck, badge: 3 },
		{ href: '/', label: 'View site', icon: Globe, exact: true }
	];

	const staff: SidebarGroup[] = [
		{
			items: [
				{ href: '/admin', label: 'Overview', icon: LayoutDashboard, exact: true },
				{ href: '/admin/requests', label: 'Requests', icon: ClipboardCheck, badge: 3 },
				{ href: '/admin/projects', label: 'Projects', icon: Disc3 },
				{ href: '/admin/audit', label: 'Audit log', icon: ScrollText },
				{ href: '/admin/system', label: 'System', icon: Activity }
			]
		}
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet content()}
	<div class="flex flex-col gap-4">
		<h1 class="m-0 text-title font-semibold">Neon Tide</h1>
		<p class="m-0 text-fg-muted">The page sits here, beside the sidebar.</p>
		<div class="h-60 rounded-xl border border-line bg-surface"></div>
	</div>
{/snippet}

<Story name="Dashboard" asChild>
	<SidebarLayout label="Dashboard" title="Dashboard" groups={dashboard(listed)} {footer} current="/dashboard/projects/neon-tide">{@render content()}</SidebarLayout>
</Story>

<Story name="Dashboard, projects loading" asChild>
	<SidebarLayout label="Dashboard" title="Dashboard" groups={dashboard(new Promise(() => {}))} footer={footer.slice(1)} current="/dashboard">{@render content()}</SidebarLayout>
</Story>

<Story name="Staff console" asChild>
	<SidebarLayout label="Staff" title="Admin" groups={staff} current="/admin/requests">{@render content()}</SidebarLayout>
</Story>

<Story name="Phone" asChild globals={phone}>
	<SidebarLayout label="Dashboard" title="Dashboard" groups={dashboard(listed)} {footer} current="/dashboard/bookings">{@render content()}</SidebarLayout>
</Story>

<Story name="Phone, menu open" asChild globals={phone}>
	<SidebarLayout label="Dashboard" title="Dashboard" groups={dashboard(listed)} {footer} current="/dashboard/bookings" open>{@render content()}</SidebarLayout>
</Story>
