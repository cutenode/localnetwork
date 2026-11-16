<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ComponentProps } from 'svelte';
	import { claimTerms } from '#lib/requests.ts';
	import ClaimFields from './ClaimFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Claim fields', component: ClaimFields, tags: ['autodocs'], render: template });

	const claimant = { did: 'did:plc:ewvi7nxzyoun6zhxrhs64oiz', handle: 'neontide.bsky.social' };

	const values = { relationship: 'self', evidence: 'I added it to our SoundCloud bio.', links: 'https://soundcloud.com/neontide' };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** A venue's claim, which only an owner makes, so it doesn't ask how they're connected. */
	const venue = claimTerms({ space: 'at://did:plc:root/computer.localnetwork.space.venue/3tresor' });
</script>

<!-- The claim pages lay the fields out in a card. -->
{#snippet template(args: ComponentProps<typeof ClaimFields>)}
	<div class="card flex max-w-2xl flex-col gap-5 p-5 sm:p-6"><ClaimFields {...args} /></div>
{/snippet}

<Story name="Empty" args={{ values: {}, claimant }} />

<Story name="Filled" args={{ values, claimant }} />

<Story name="No handle" args={{ values: {}, claimant: { ...claimant, handle: null } }} />

<Story name="Phone, empty" args={{ values: {}, claimant }} globals={phone} />

<Story name="Phone, no handle" args={{ values: {}, claimant: { ...claimant, handle: null } }} globals={phone} />

<Story name="Venue, empty" args={{ values: {}, claimant, terms: venue }} />

<Story name="Venue, filled" args={{ values: { evidence: 'The team page on our site lists me as owner.', links: 'https://tresor.example/team' }, claimant, terms: venue }} />

<Story name="Venue, phone" args={{ values: {}, claimant, terms: venue }} globals={phone} />
