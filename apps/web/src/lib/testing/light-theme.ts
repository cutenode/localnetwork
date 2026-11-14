/** A story's `beforeEach` that draws it in the light theme, as the theme toggle does, and puts the dark one back after. */
export const lightTheme = () => {
	document.documentElement.classList.add('light');

	return () => document.documentElement.classList.remove('light');
};
