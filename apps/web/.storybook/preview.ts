import type { Preview } from '@storybook/sveltekit';
import { themes } from 'storybook/theming';
import './preview.css';

const preview: Preview = {
	parameters: {
		layout: 'padded',
		controls: { matchers: { color: /(background|color)$/i, date: /Date$/i } },
		docs: { theme: themes.dark },
		// Accessibility is part of done: a violation fails the story's test.
		a11y: { test: 'error' }
	}
};

export default preview;
