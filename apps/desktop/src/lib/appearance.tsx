/**
 * Appearance customisation.
 *
 * Every visual property in the app is a CSS custom property, so retheming means
 * writing a handful of values onto the document root. Preferences persist to
 * localStorage and are re-applied on load before the first paint of the shell.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Icon } from "../components/icons";
import { Segmented, Switch } from "../components/ui";
import { useTheme, type ThemeMode } from "./theme";

const STORAGE_KEY = "apocrypha.appearance";

export interface Appearance {
  /** Accent hue in degrees. */
  accentHue: number;
  /** Accent saturation as a percentage. */
  accentSat: number;
  /** Second hue for the accent gradient. */
  accentHue2: number;
  /** Base font size in px; the whole type scale derives from it. */
  baseSize: number;
  /** Corner rounding multiplier applied to the radius tokens. */
  radiusScale: number;
  /** Density multiplier applied to the spacing tokens. */
  density: number;
  /** Strength of the ambient radial wash, 0 turns it off. */
  ambient: number;
  /** Whether the accent gradient is used, or a flat accent. */
  gradient: boolean;
  /** Reduce all motion regardless of the OS setting. */
  reduceMotion: boolean;
}

export const DEFAULT_APPEARANCE: Appearance = {
  accentHue: 158,
  accentSat: 38,
  accentHue2: 138,
  baseSize: 14,
  radiusScale: 1,
  density: 1,
  ambient: 1,
  gradient: true,
  reduceMotion: false,
};

/** Preset accents. Kept deliberately muted; none of them are neon. */
export const ACCENT_PRESETS: { name: string; hue: number; sat: number; hue2: number }[] =
  [
    { name: "Pine", hue: 158, sat: 38, hue2: 138 },
    { name: "Moss", hue: 128, sat: 34, hue2: 108 },
    { name: "Teal", hue: 180, sat: 36, hue2: 196 },
    { name: "Slate", hue: 210, sat: 24, hue2: 226 },
    { name: "Clay", hue: 24, sat: 40, hue2: 8 },
    { name: "Plum", hue: 292, sat: 30, hue2: 316 },
  ];

function read(): Appearance {
  if (typeof localStorage === "undefined") return DEFAULT_APPEARANCE;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_APPEARANCE;
    return { ...DEFAULT_APPEARANCE, ...(JSON.parse(raw) as Partial<Appearance>) };
  } catch {
    return DEFAULT_APPEARANCE;
  }
}

/** Write the appearance onto the document root as CSS custom properties. */
export function applyAppearance(a: Appearance) {
  const root = document.documentElement;
  const s = root.style;

  s.setProperty("--accent-h", String(a.accentHue));
  s.setProperty("--accent-s", `${a.accentSat}%`);
  s.setProperty("--accent-h2", String(a.gradient ? a.accentHue2 : a.accentHue));

  // Type scale derives from the base size, staying on even values.
  const base = a.baseSize;
  s.setProperty("--text-base", `${base}px`);
  s.setProperty("--text-xs", `${Math.round(base - 2)}px`);
  s.setProperty("--text-sm", `${Math.round(base - 1)}px`);
  s.setProperty("--text-md", `${base}px`);
  s.setProperty("--text-lg", `${Math.round(base + 2)}px`);
  s.setProperty("--text-xl", `${Math.round(base * 1.45)}px`);
  s.setProperty("--text-2xl", `${Math.round(base * 1.75)}px`);

  // Spacing and radius stay on the power-of-two rhythm, scaled as a whole.
  const sp = [2, 4, 8, 16, 32, 64, 128];
  sp.forEach((v, i) => {
    s.setProperty(`--sp-${i + 1}`, `${Math.round(v * a.density)}px`);
  });
  s.setProperty("--radius-xs", `${Math.round(4 * a.radiusScale)}px`);
  s.setProperty("--radius-sm", `${Math.round(8 * a.radiusScale)}px`);
  s.setProperty("--radius", `${Math.round(8 * a.radiusScale)}px`);
  s.setProperty("--radius-lg", `${Math.round(16 * a.radiusScale)}px`);
  s.setProperty("--radius-xl", `${Math.round(16 * a.radiusScale)}px`);

  s.setProperty("--ambient-opacity", String(a.ambient));

  if (a.reduceMotion) {
    s.setProperty("--dur-fast", "0.001ms");
    s.setProperty("--dur", "0.001ms");
    s.setProperty("--dur-slow", "0.001ms");
  } else {
    s.removeProperty("--dur-fast");
    s.removeProperty("--dur");
    s.removeProperty("--dur-slow");
  }
}

export function useAppearance() {
  const [appearance, setAppearance] = useState<Appearance>(read);

  useEffect(() => {
    applyAppearance(appearance);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(appearance));
    } catch {
      /* Storage can be unavailable; the theme still applies this session. */
    }
  }, [appearance]);

  const set = useCallback(<K extends keyof Appearance>(key: K, value: Appearance[K]) => {
    setAppearance((prev) => ({ ...prev, [key]: value }));
  }, []);

  const reset = useCallback(() => setAppearance(DEFAULT_APPEARANCE), []);

  return useMemo(() => ({ appearance, set, reset }), [appearance, set, reset]);
}

/** Apply saved preferences as early as possible, before React mounts. */
export function bootAppearance() {
  applyAppearance(read());
}

/* ----------------------------------------------------------------- panel -- */

export function AppearancePanel({
  appearance,
  set,
  reset,
}: ReturnType<typeof useAppearance>) {
  const { mode, setMode } = useTheme();

  return (
    <div className="card stack">
      <div className="row">
        <div className="card-title">Appearance</div>
        <div style={{ marginLeft: "auto" }}>
          <button className="btn sm ghost" onClick={reset}>
            <Icon.refresh size={14} /> Reset
          </button>
        </div>
      </div>

      <div className="field">
        <label>Theme</label>
        <Segmented
          idPrefix="theme"
          value={mode}
          onChange={(v) => setMode(v as ThemeMode)}
          options={[
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
            { value: "system", label: "Match system" },
          ]}
        />
      </div>

      <div className="field">
        <label>Accent colour</label>
        <div className="swatches">
          {ACCENT_PRESETS.map((p) => {
            const active =
              appearance.accentHue === p.hue && appearance.accentSat === p.sat;
            return (
              <button
                key={p.name}
                className={`swatch ${active ? "active" : ""}`}
                title={p.name}
                aria-label={p.name}
                onClick={() => {
                  set("accentHue", p.hue);
                  set("accentSat", p.sat);
                  set("accentHue2", p.hue2);
                }}
                style={{
                  background: `linear-gradient(135deg, hsl(${p.hue} ${p.sat}% 44%), hsl(${p.hue2} ${p.sat - 4}% 38%))`,
                }}
              />
            );
          })}
        </div>
      </div>

      <div className="field">
        <label>
          Colour intensity <Value>{appearance.accentSat}%</Value>
        </label>
        <input
          className="range"
          type="range"
          min={12}
          max={72}
          step={2}
          value={appearance.accentSat}
          onChange={(e) => set("accentSat", Number(e.target.value))}
        />
      </div>

      <div className="field">
        <label>
          Text size <Value>{appearance.baseSize}px</Value>
        </label>
        <input
          className="range"
          type="range"
          min={12}
          max={18}
          step={1}
          value={appearance.baseSize}
          onChange={(e) => set("baseSize", Number(e.target.value))}
        />
      </div>

      <div className="field">
        <label>
          Corner rounding <Value>{Math.round(appearance.radiusScale * 100)}%</Value>
        </label>
        <input
          className="range"
          type="range"
          min={0}
          max={2}
          step={0.25}
          value={appearance.radiusScale}
          onChange={(e) => set("radiusScale", Number(e.target.value))}
        />
      </div>

      <div className="field">
        <label>
          Spacing <Value>{Math.round(appearance.density * 100)}%</Value>
        </label>
        <input
          className="range"
          type="range"
          min={0.75}
          max={1.5}
          step={0.25}
          value={appearance.density}
          onChange={(e) => set("density", Number(e.target.value))}
        />
      </div>

      <div className="field">
        <label>
          Background glow <Value>{Math.round(appearance.ambient * 100)}%</Value>
        </label>
        <input
          className="range"
          type="range"
          min={0}
          max={1.5}
          step={0.25}
          value={appearance.ambient}
          onChange={(e) => set("ambient", Number(e.target.value))}
        />
      </div>

      <div className="divider" />

      <div className="row">
        <Switch
          checked={appearance.gradient}
          onChange={(v) => set("gradient", v)}
          label="Use a gradient accent"
        />
        <span>Gradient accent</span>
        <span className="card-hint" style={{ marginLeft: "auto" }}>
          Blends two hues on buttons and highlights
        </span>
      </div>

      <div className="row">
        <Switch
          checked={appearance.reduceMotion}
          onChange={(v) => set("reduceMotion", v)}
          label="Reduce motion"
        />
        <span>Reduce motion</span>
        <span className="card-hint" style={{ marginLeft: "auto" }}>
          Turns off animation regardless of system settings
        </span>
      </div>
    </div>
  );
}

function Value({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        color: "var(--text-tertiary)",
        fontWeight: 400,
        fontVariantNumeric: "tabular-nums",
        marginLeft: "var(--sp-3)",
      }}
    >
      {children}
    </span>
  );
}
