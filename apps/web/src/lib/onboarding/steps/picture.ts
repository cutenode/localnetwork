import type { StepDefinition } from '../checklist';

export default {
	order: 20,
	title: 'Add a picture',
	gets: 'People spot you in lists and lineups.',
	href: '/dashboard/profile#avatar',
	done: ({ profile }) => !!profile?.basics.avatar
} satisfies StepDefinition;
