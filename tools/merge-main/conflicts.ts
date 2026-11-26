// What main's renames do to a text, undone: `prop_` before a name, `.page-only` after a module's name.
const unrenamed = (text: string) => text.replace(/\bprop_(?=[A-Za-z_])/g, '').replace(/\.page-only(?![\w-])/g, '');

/**
 * Whether main's side of a conflict differs from the merge base only by the renames, so this
 * branch's side, renamed the same way afterwards, loses nothing of main's.
 */
export const renameOnly = (base: string, theirs: string) => unrenamed(base) === unrenamed(theirs);
