import type { StepDefinition } from '../checklist';

export default {
	order: 40,
	title: 'Add a bio and links',
	gets: 'Says who you are and where else to find you.',
	href: '/dashboard/profile#bio',
	done: ({ profile }) => !!profile?.basics.bio && !!profile.basics.links?.length
} satisfies StepDefinition;
