import type { HueSat, Mode } from "./types";

/** CSS color for a hue (0–360) and saturation (0–100) at full value. */
export function hueSatCss({ hue, saturation }: HueSat): string {
  const s = saturation / 100;
  const lightness = 1 - s / 2;
  const sl = lightness === 0 || lightness === 1 ? 0 : (1 - lightness) / Math.min(lightness, 1 - lightness);
  return `hsl(${hue} ${(sl * 100).toFixed(1)}% ${(lightness * 100).toFixed(1)}%)`;
}

/** Approximate sRGB color of a black-body light source (Tanner Helland's fit). */
export function kelvinRgb(kelvin: number): [number, number, number] {
  const t = kelvin / 100;
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  const r = t <= 66 ? 255 : 329.698727446 * Math.pow(t - 60, -0.1332047592);
  const g = t <= 66 ? 99.4708025861 * Math.log(t) - 161.1195681661 : 288.1221695283 * Math.pow(t - 60, -0.0755148492);
  const b = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  return [clamp(r), clamp(g), clamp(b)];
}

export function kelvinCss(kelvin: number): string {
  const [r, g, b] = kelvinRgb(kelvin);
  return `rgb(${r} ${g} ${b})`;
}

export function lightCss(light: { mode: Mode; hue: number; saturation: number; temperature: number }): string {
  return light.mode === "color" ? hueSatCss(light) : kelvinCss(light.temperature);
}

export const TEMPERATURE_TRACK = `linear-gradient(90deg, ${[2500, 3200, 4000, 5000, 6500]
  .map((k) => kelvinCss(k))
  .join(", ")})`;
