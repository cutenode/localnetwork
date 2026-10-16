import { addons } from 'storybook/manager-api';
import { create } from 'storybook/theming';

addons.setConfig({
	theme: create({
		base: 'dark',
		brandTitle: 'Local Network',
		appBg: '#09090b',
		appContentBg: '#09090b',
		barBg: '#0e0e11'
	})
});
