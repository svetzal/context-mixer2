// Rewrite relative `.ts` import/export specifiers to `.js` in the emitted
// declaration files.
//
// The source uses Bun-idiomatic explicit `.ts` extensions. `tsc`'s
// `rewriteRelativeImportExtensions` rewrites those to `.js` in the emitted
// JavaScript, but leaves them as `.ts` in the *type-only* imports it preserves
// into the `.d.ts` output — dangling references, since only `.d.ts`/`.js` ship.
// This normalizes the declarations to point at `.js` (which TypeScript resolves
// to the sibling `.d.ts` for types), so consumers' typecheckers resolve cleanly.
//
// Not shipped: `files` limits the published tarball to `dist/`.

import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const distDir = path.join(import.meta.dir, "..", "dist");
const relativeTsSpecifier = /(\bfrom\s*")(\.[^"]*?)\.ts(")/g;

const entries = await readdir(distDir, { recursive: true });
let rewritten = 0;

for (const entry of entries) {
  if (!entry.endsWith(".d.ts")) continue;
  const full = path.join(distDir, entry);
  const source = await readFile(full, "utf8");
  const output = source.replace(relativeTsSpecifier, "$1$2.js$3");
  if (output !== source) {
    await writeFile(full, output);
    rewritten += 1;
  }
}

console.log(`fix-dist-dts: normalized ${rewritten} declaration file(s)`);
