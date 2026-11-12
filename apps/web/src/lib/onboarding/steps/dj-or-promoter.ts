import type { StepDefinition } from '../checklist';

export default {
	order: 50,
	title: 'Say if you DJ or put on shows',
	gets: 'Adds what promoters need to book you, or the cities you book in.',
	href: '/dashboard/profile#section-performer',
	done: ({ profile }) => !!(profile?.performer ?? profile?.promoter)
} satisfies StepDefinition;
