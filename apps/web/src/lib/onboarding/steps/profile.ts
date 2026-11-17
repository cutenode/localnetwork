import ProfileSetupForm from '#lib/components/ProfileSetupForm.svelte';
import type { StepDefinition } from '../checklist';

export default {
	order: 10,
	title: 'Make your profile',
	gets: 'Your own page, the one people follow and book.',
	href: '/dashboard/profile',
	done: ({ profile }) => profile !== null,
	Form: ProfileSetupForm
} satisfies StepDefinition;
