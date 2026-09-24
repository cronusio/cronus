import { describe, expect, it } from "vitest";
import {
  DEFAULT_SCHEME_ID,
  registerScheme,
  resolveScheme,
  resolveTheme,
  type SchemeManifest,
  schemeCatalog,
  surfaceAttributes,
  themeAttributes,
} from "./theme";

describe("mode axis (unchanged)", () => {
  it("resolves system against the OS preference; explicit choices pass through", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("themeAttributes derives, never a literal colour", () => {
    expect(themeAttributes("dark")).toEqual({
      "data-theme": "dark",
      className: "dark",
    });
    expect(themeAttributes("light")).toEqual({
      "data-theme": "light",
      className: "",
    });
  });
});

describe("scheme axis · resolveScheme (mode × scheme)", () => {
  it("system + default resolves to the OS-preferred variant of the default scheme", () => {
    expect(resolveScheme("system", "default", true)).toEqual({
      resolvedMode: "dark",
      schemeId: "default",
    });
    expect(resolveScheme("system", "default", false)).toEqual({
      resolvedMode: "light",
      schemeId: "default",
    });
  });

  it("an explicit mode overrides the OS preference", () => {
    expect(resolveScheme("light", "default", true).resolvedMode).toBe("light");
  });

  it("an unknown scheme id falls back to default with a surfaced warning (never blank)", () => {
    const r = resolveScheme("dark", "nonexistent", true);
    expect(r.schemeId).toBe("default");
    expect(r.resolvedMode).toBe("dark");
    expect(r.warning).toMatch(/nonexistent/);
    expect(r.integrityError).toBeUndefined();
  });

  it("a registered scheme id resolves as-requested (id-stable catalog)", () => {
    const midnight: SchemeManifest = {
      id: "midnight",
      name: "Midnight",
      category: "Brand",
      provenance: {
        kind: "local",
        reference: "test",
      },
      fidelity: "hybrid",
      files: {
        light: "tokens.light.css",
        dark: "tokens.dark.css",
      },
    };
    registerScheme(midnight);
    expect(schemeCatalog().map((s) => s.id)).toContain("midnight");
    expect(resolveScheme("dark", "midnight", true)).toEqual({
      resolvedMode: "dark",
      schemeId: "midnight",
    });
  });
});

describe("scheme axis · surfaceAttributes (root application, cosmetic-only)", () => {
  it("produces data-theme + data-scheme + the dark class for a resolved pair", () => {
    const a = surfaceAttributes("dark", "default", true);
    expect(a["data-theme"]).toBe("dark");
    expect(a["data-scheme"]).toBe("default");
    expect(a.className).toBe("dark");
  });

  it("swapping either axis only changes attributes — no literal colours", () => {
    const dark = surfaceAttributes("dark", "default", true);
    const light = surfaceAttributes("light", "default", true);
    expect(dark["data-theme"]).not.toBe(light["data-theme"]);
    expect(light.className).toBe("");
    expect(
      JSON.stringify({
        dark,
        light,
      }),
    ).not.toMatch(/#[0-9a-f]{3}/i);
  });

  it("carries the fallback verdict through on an unknown scheme", () => {
    const a = surfaceAttributes("dark", "bogus", false);
    expect(a["data-scheme"]).toBe(DEFAULT_SCHEME_ID);
    expect(a.resolved.warning).toBeDefined();
  });
});
