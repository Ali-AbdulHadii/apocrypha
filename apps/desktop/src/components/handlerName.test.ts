/**
 * Naming whoever owns `nxm://`.
 *
 * The backend reports what the system holds, and the two systems hold very
 * different things — a desktop entry id on Linux, a full command line on
 * Windows. Both end up in the same sentence in Settings, so getting this wrong
 * shows a user a registry command where a program name should be.
 */

import { describe, expect, it } from "vitest";
import { handlerName } from "./SettingsScreen";

describe("naming a Windows handler", () => {
  it("takes the program out of a command line", () => {
    // The real value on a machine with Vortex installed. Nobody should have to
    // read this to learn that Vortex has the scheme.
    expect(
      handlerName(
        '"C:\\Program Files\\Black Tree Gaming Ltd\\Vortex\\Vortex.exe" -d "%1"',
      ),
    ).toBe("Vortex");
  });

  it("handles a command with no quotes", () => {
    expect(handlerName("C:\\Apps\\other.exe %1")).toBe("other");
  });

  it("keeps a name that has spaces inside the quotes", () => {
    expect(handlerName('"C:\\Apps\\Mod Manager.exe" "%1"')).toBe("Mod Manager");
  });
});

describe("naming a Linux handler", () => {
  it("drops the desktop suffix", () => {
    expect(handlerName("dev.apocrypha.desktop-manager.desktop")).toBe(
      "dev.apocrypha.desktop-manager",
    );
    expect(handlerName("com.nexusmods.app.desktop")).toBe("com.nexusmods.app");
  });
});

describe("input worth not crashing on", () => {
  it("falls back to what it was given rather than an empty chip", () => {
    // A chip reading "opens them" with nothing before it is worse than a chip
    // reading something odd.
    expect(handlerName("something-unrecognised")).toBe("something-unrecognised");
    expect(handlerName("  spaced.desktop  ")).toBe("spaced");
  });

  it("survives an empty value", () => {
    expect(() => handlerName("")).not.toThrow();
  });
});
