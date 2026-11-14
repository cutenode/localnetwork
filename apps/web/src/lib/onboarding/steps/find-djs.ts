import type { StepDefinition } from '../checklist';

export default {
	order: 60,
	title: 'Ask to be listed in Find DJs',
	gets: 'Promoters searching Find DJs find you.',
	href: '/dashboard/profile#listing',
	applies: ({ profile }) => !!profile?.performer,
	done: ({ listing }) => listing
} satisfies StepDefinition;
