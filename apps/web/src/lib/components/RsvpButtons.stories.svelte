<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { light } from '#lib/testing/light.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import RsvpButtons from './RsvpButtons.svelte';

	/**
	 * Going and Interested on their own, in the states Pages/Event doesn't draw: signed in with no answer yet and a
	 * single person counted, Interested pressed, and an answer refused for a reason signing in again won't fix. Each at
	 * the default width in the dark palette and at 390px in the light one.
	 */
	const { Story } = defineMeta({ title: 'Patterns/Going and Interested', component: RsvpButtons });

	/** Stands in for SvelteKit's form sending, which a story doesn't have. */
	const harness = { did: 'did:plc:neon', next: '/party/3lzparty/3mwinter/winter-opening', enhance: () => ({ destroy: () => {} }) };

	const unanswered = { ...harness, rsvp: { going: 0, interested: 1, answer: null } };

	const interested = { ...harness, rsvp: { going: 48, interested: 112, answer: 'interested' as const } };

	const refused = {
		...unanswered,
		refused: { error: "This event isn't taking answers anymore", reauth: false }
	};

	const button = (canvasElement: HTMLElement, name: RegExp) => within(canvasElement).getByRole('button', { name });

	/** Neither is pressed, and the one person interested reads as a person, not people. */
	const noAnswer = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(button(canvasElement, /^Going/)).toHaveAttribute('aria-pressed', 'false');
		await expect(button(canvasElement, /^Interested/)).toHaveAccessibleName(/1 person$/);
	};

	/** Interested is pressed, and pressing it again sends no answer, which takes it back. */
	const pressedInterested = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(button(canvasElement, /^Interested/)).toHaveAttribute('aria-pressed', 'true');
		await expect(button(canvasElement, /^Interested/)).toHaveValue('');
		await expect(button(canvasElement, /^Going/)).toHaveValue('going');
	};

	/** The refusal shows under the buttons, with no Sign in again, since signing in again wouldn't help. */
	const showsRefusal = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByText("This event isn't taking answers anymore")).toBeInTheDocument();
		await expect(canvas.queryByRole('button', { name: /sign in again/i })).not.toBeInTheDocument();
	};
</script>

<Story name="Not answered yet" args={unanswered} play={noAnswer} />

<Story name="Not answered yet, phone, light" args={unanswered} globals={phone} beforeEach={light} play={noAnswer} />

<Story name="Interested" args={interested} play={pressedInterested} />

<Story name="Interested, phone, light" args={interested} globals={phone} beforeEach={light} play={pressedInterested} />

<Story name="Refused" args={refused} play={showsRefusal} />

<Story name="Refused, phone, light" args={refused} globals={phone} beforeEach={light} play={showsRefusal} />
