/**
 * Characters the cards' fonts can't draw, which Satori would draw as empty boxes: scripts other than Latin, and emoji
 * (with the joiners and variation selectors that build them), but not the ©, ® and ™ the fonts have.
 */
export const UNDRAWABLE =
	/[^\p{Script=Latin}\p{Script=Common}\p{Script=Inherited}]|(?![©®™])\p{Extended_Pictographic}|‍|️/gu;

/** Whether a card could draw any of `text`, so a name written only in another script gets the site's card instead. */
export const isDrawable = (text: string) => /[\p{L}\p{N}]/u.test(text.replace(UNDRAWABLE, ''));
