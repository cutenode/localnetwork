import type { StepDefinition } from '../checklist';

export default {
	order: 30,
	title: 'Set your home city',
	gets: 'Promoters searching near them find you.',
	href: '/dashboard/profile',
	done: ({ profile }) => !!profile?.basics.location
} satisfies StepDefinition;
