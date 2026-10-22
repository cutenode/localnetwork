import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

// The Local Network theme names its font sizes (packages/tokens/theme.css). Without this,
// tailwind-merge takes `text-caption` for a colour and drops the `text-fg-inverse` before it.
const twMerge = extendTailwindMerge({
	extend: {
		classGroups: {
			'font-size': [
				{ text: ['caption', 'label', 'small', 'body', 'emphasis', 'lede', 'intro', 'heading', 'title', 'display'] }
			]
		}
	}
});

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}
