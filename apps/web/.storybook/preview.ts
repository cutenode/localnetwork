import type { Preview } from '@storybook/sveltekit';
import { themes } from 'storybook/theming';
import { enhanced } from '../src/lib/testing/submits.ts';
import { VIEWPORTS } from '../src/lib/testing/viewports.ts';
import './preview.css';

const preview: Preview = {
	parameters: {
		layout: 'padded',
		controls: { matchers: { color: /(background|color)$/i, date: /Date$/i } },
		docs: { theme: themes.dark },
		viewport: { options: VIEWPORTS },
		// A story's own sveltekit_experimental parameters merge with this, so every form's submit reaches the spy.
		sveltekit_experimental: { forms: { enhance: enhanced } },
		// Accessibility is part of done: a violation fails the story's test. axe's default run leaves out
		// WCAG 2.2's rules (target-size), so this names every level it checks, best practices included.
		a11y: {
			test: 'error',
			options: {
				runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] }
			}
		}
	}
};

export default preview;
