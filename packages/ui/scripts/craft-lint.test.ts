import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { describe, expect, it } from "vitest";

// vitest runs with cwd = packages/ui.
const script = join(process.cwd(), "scripts", "craft-lint.mjs");

function runLint(file: string): {
  code: number;
  out: string;
} {
  try {
    const out = execFileSync(
      "node",
      [
        script,
        file,
      ],
      {
        encoding: "utf8",
      },
    );
    return {
      code: 0,
      out,
    };
  } catch (err) {
    const e = err as {
      status?: number;
      stderr?: string;
      stdout?: string;
    };
    return {
      code: e.status ?? 1,
      out: `${e.stdout ?? ""}${e.stderr ?? ""}`,
    };
  }
}

function tempFile(name: string, body: string): string {
  const dir = mkdtempSync(join(tmpdir(), "craft-lint-"));
  const p = join(dir, name);
  writeFileSync(p, body, "utf8");
  return p;
}

describe("craft lint (must-fix subset)", () => {
  it("fails a build on a literal colour in a component", () => {
    const file = tempFile(
      "bad.tsx",
      'export const X = () => <div style={{ color: "#ff0000" }} />;\n',
    );
    const r = runLint(file);
    expect(r.code).toBe(1);
    expect(r.out).toMatch(/literal-colour/);
  });

  it("fails on a literal font-family and a raw-px border-radius", () => {
    const font = tempFile("f.tsx", 'const s = "font-family: Inter";\n');
    const radius = tempFile("r.tsx", 'const s = "border-radius: 8px";\n');
    expect(runLint(font).code).toBe(1);
    expect(runLint(radius).code).toBe(1);
  });

  it("passes a token-only component", () => {
    const file = tempFile(
      "ok.tsx",
      'export const X = () => <div className="bg-surface-1 text-text-muted rounded-md" />;\n',
    );
    const r = runLint(file);
    expect(r.code).toBe(0);
    expect(r.out).toMatch(/clean/);
  });
});
