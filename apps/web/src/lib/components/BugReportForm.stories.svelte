<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import BugReportForm from './BugReportForm.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Bug report', component: BugReportForm, tags: ['autodocs'] });

	const github = 'https://github.com/cutenode/localnetwork/issues/new?template=web-bug.yml&steps=1.+Open+%2Fmatch&browser=Chrome+141%2C+macOS';

	const facts = { page: '/match', browser: 'Chrome 141, macOS' };

	const values = { title: "The map doesn't load on a show", body: 'I added a venue to a show and the map stayed grey. Reloading the page did nothing.' };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Approve posting" args={{ step: { kind: 'consent' }, github, facts }} />

<Story name="Form" args={{ step: { kind: 'form' }, github, facts }} />

<Story name="Posting" args={{ step: { kind: 'form' }, github, facts, values, sending: true }} />

<Story name="Could not post" args={{ step: { kind: 'form' }, github, facts, values, error: "We couldn't post your report." }} />

<Story name="Posted" args={{ step: { kind: 'sent', url: 'https://userinput.app/d/did:plc:abc/3mabc' }, github, facts }} />

<Story name="Phone, approve posting" args={{ step: { kind: 'consent' }, github, facts }} globals={phone} />

<Story name="Phone, form" args={{ step: { kind: 'form' }, github, facts, values }} globals={phone} />

<Story name="Phone, could not post" args={{ step: { kind: 'form' }, github, facts, values, error: "We couldn't post your report." }} globals={phone} />

<Story name="Phone, posted" args={{ step: { kind: 'sent', url: 'https://userinput.app/d/did:plc:abc/3mabc' }, github, facts }} globals={phone} />
