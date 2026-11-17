<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import FormSection from './FormSection.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Form section', component: FormSection, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet fields()}
	<div>
		<label class="label" for="bookingEmail">Booking email</label>
		<input id="bookingEmail" class="input" type="email" name="bookingEmail" value="bookings@neontide.test" />
	</div>
	<div>
		<label class="label" for="rider">Rider</label>
		<textarea id="rider" class="input min-h-24 py-2.5" name="rider">Two CDJ-3000s, a DJM-V10, and booth monitors.</textarea>
	</div>
{/snippet}

<Story name="With fields" asChild>
	<FormSection title="Private booking details" description="Only this project's managers and Local Network staff see these.">{@render fields()}</FormSection>
</Story>

<Story name="Phone" globals={phone} asChild>
	<FormSection title="Private booking details" description="Only this project's managers and Local Network staff see these, and people you let into one of your press kits.">{@render fields()}</FormSection>
</Story>
