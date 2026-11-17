import type { StepDefinition } from '../checklist';

export default {
	order: 70,
	title: 'Follow a DJ',
	gets: 'Their shows show up under Following.',
	href: '/djs',
	done: ({ follows }) => follows
} satisfies StepDefinition;
