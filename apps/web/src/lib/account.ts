import { Bell, ClipboardList, Database, History, type LucideIcon } from '@lucide/svelte';

/** The pages of the Account area, in order. `about` says what one holds, for when its own line can't be read. */
export const ACCOUNT_PAGES: { href: string; label: string; icon: LucideIcon; about: string }[] = [
	{ href: '/dashboard/account/notifications', label: 'Notifications', icon: Bell, about: 'Email and Bluesky DMs' },
	{ href: '/dashboard/account/activity', label: 'Activity', icon: History, about: "What you've done here" },
	{ href: '/dashboard/account/data', label: 'Your data', icon: Database, about: 'What Local Network holds for you' },
	{
		href: '/dashboard/account/requests',
		label: 'Requests to staff',
		icon: ClipboardList,
		about: "What you've asked staff for"
	}
];
