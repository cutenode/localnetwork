// `node tools/labeler/cli.ts <file>`, from the repository root: writes the labeler config
// (labeler.ts) to <file>. pr-labels.yml runs it before actions/labeler.
import { writeFileSync } from 'node:fs';
import { repoConfig } from './labeler.ts';

writeFileSync(process.argv[2], repoConfig());
