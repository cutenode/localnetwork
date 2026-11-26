// `pnpm merge:main`, from the repository root, while merging main into a branch from before main's
// renames: after `git merge origin/main` stops on a conflict, or with `--no-commit`. merge-main.ts
// says what it does.
import { mergeMain } from './merge-main.ts';

process.exitCode = mergeMain(process.cwd(), (line) => process.stdout.write(`${line}\n`)) ? 0 : 1;
