<script lang="ts">
	import { page } from '$app/state';
	import { Activity, BadgeCheck, ClipboardCheck, Disc3, LayoutDashboard, ScrollText, ShieldCheck } from '@lucide/svelte';
	import SidebarLayout from '#lib/ui/SidebarLayout.svelte';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const groups = $derived([
		{
			items: [
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
			]
		}
	]);
</script>

<SidebarLayout label="Staff" title={data.viewer?.isAdmin ? 'Admin' : 'Moderator'} {groups} current={page.url.pathname}>{@render children()}</SidebarLayout>
