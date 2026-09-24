#!/usr/bin/env node
/*
 * Craft lint — the mechanically-enforced must-fix subset of the design-identity
 * craft bar: every visual value in a component derives from a
 * token, never a literal. Advisory craft rules are surfaced separately and do
 * not fail the run; the auto-vs-advisory split is the config below, not code.
 *
 * Usage:
 *   node scripts/craft-lint.mjs                 # lint packages/ui/src
 *   node scripts/craft-lint.mjs <file> [file…]  # lint specific files
 *
 * Exit 0 = clean · exit 1 = at least one must-fix violation.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const SRC = join(ROOT, "src");

/** Files exempt from the token contract: the token layer itself and tests. */
const EXEMPT = [
  /[\\/]tokens\.(ts|css)$/,
  /[\\/]schemes[\\/]/,
  /[\\/]craft-lint\./,
  /\.test\.(ts|tsx)$/,
];

/** must-fix rules — a match fails the build. */
const MUST_FIX = [
  {
    id: "literal-colour",
    // hex, rgb(), rgba(), hsl(), hsla() appearing in a component source
    re: /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g,
    msg: "literal colour outside the token layer — use var(--…) or a mapped Tailwind utility",
  },
  {
    id: "literal-font-family",
    re: /font-family\s*:/g,
    msg: "literal font-family — bind --font-sans / --font-mono",
  },
  {
    id: "raw-px-radius",
    re: /border-radius\s*:\s*\d/g,
    msg: "raw-px border-radius — use --radius-sm/md/lg/pill",
  },
];

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      out.push(...walk(p));
    } else if (/\.(ts|tsx)$/.test(name)) {
      out.push(p);
    }
  }
  return out;
}

const targets = process.argv.length > 2 ? process.argv.slice(2) : walk(SRC);

let violations = 0;
for (const file of targets) {
  if (EXEMPT.some((re) => re.test(file))) continue;
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  for (const rule of MUST_FIX) {
    lines.forEach((line, i) => {
      // Ignore the rule's own examples inside a line comment.
      if (line.trimStart().startsWith("//") || line.trimStart().startsWith("*")) return;
      rule.re.lastIndex = 0;
      if (rule.re.test(line)) {
        const rel = relative(ROOT, file).replace(/\\/g, "/");
        console.error(`  ${rel}:${i + 1}  [${rule.id}]  ${rule.msg}`);
        violations += 1;
      }
    });
  }
}

if (violations > 0) {
  console.error(`\ncraft-lint: ${violations} must-fix violation(s)`);
  process.exit(1);
}
console.log("craft-lint: clean");
