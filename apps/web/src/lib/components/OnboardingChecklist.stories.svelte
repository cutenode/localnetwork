<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { collect, type OnboardingData, STEPS } from '#lib/onboarding/checklist.ts';
	import OnboardingChecklist from './OnboardingChecklist.svelte';

	const { Story } = defineMeta({
		title: 'Patterns/Onboarding checklist',
		component: OnboardingChecklist,
		tags: ['autodocs'],
		parameters: {
			viewport: {
				options: {
					phone: { name: 'Phone (390)', styles: { width: '390px', height: '844px' }, type: 'mobile' },
					wide: { name: 'Wide (1280)', styles: { width: '1280px', height: '800px' }, type: 'desktop' }
				}
			}
		}
	});

	const phone = { viewport: { value: 'phone', isRotated: false } };

	const wide = { viewport: { value: 'wide', isRotated: false } };

	const fresh: OnboardingData = { profile: null, prefill: { name: 'Pat', hasAvatar: true }, listing: false, follows: false };

	const partly: OnboardingData = {
		profile: { basics: { name: 'Pat', bio: 'Warehouse techno, Berlin.' }, performer: {}, createdAt: '', updatedAt: '' },
		prefill: null,
		listing: false,
		follows: true
	};

	const finished: OnboardingData = {
		profile: {
			basics: {
				name: 'Pat',
				bio: 'Warehouse techno, Berlin.',
				avatar: { $type: 'blob', ref: { $link: 'bafy' }, mimeType: 'image/png', size: 1 },
				location: { city: 'Berlin', country: 'DE' },
				links: [{ kind: 'website', url: 'https://pat.test' }]
			},
			performer: {},
			createdAt: '',
			updatedAt: ''
		},
		prefill: null,
		listing: true,
		follows: true
	};

	// A feature's step, as one would add it when its flag turns on for someone.
	const withFeature = [
		...STEPS,
		...collect({
			'./steps/mixes.ts': {
				default: { order: 80, title: 'Add a mix', gets: 'Promoters hear you before they book.', href: '/dashboard/profile', flag: 'parityCanary', done: () => false }
			}
		})
	];
</script>

<Story name="New account" args={{ data: fresh, flags: undefined }} globals={wide} />

<Story name="New account on a phone" args={{ data: fresh, flags: undefined }} globals={phone} />

<Story name="Partly done" args={{ data: partly, flags: undefined }} globals={wide} />

<Story name="Partly done on a phone" args={{ data: partly, flags: undefined }} globals={phone} />

<Story name="With a new feature's step" args={{ data: partly, flags: { parityCanary: true }, steps: withFeature }} globals={wide} />

<Story name="With a new feature's step on a phone" args={{ data: partly, flags: { parityCanary: true }, steps: withFeature }} globals={phone} />

<!-- Every step done: the checklist is gone. -->
<Story name="All done" args={{ data: finished, flags: undefined }} globals={wide} />
