export type Mode = "white" | "color";
export type Status = "connecting" | "online" | "offline" | "authError";
export type Theme = "system" | "light" | "dark";
export type EffectName = "breathe" | "candle" | "colorLoop" | "sunrise" | "sunset" | "alert";

export interface EffectOptions {
  period?: number;
  minutes?: number;
  brightness?: number;
  color?: HueSat;
  times?: number;
}

export interface HueSat {
  hue: number;
  saturation: number;
}

export interface Bulb {
  ip: string;
  name: string;
  room: string;
  model: string | null;
  firmware: string | null;
  status: Status;
  error: string | null;
  on: boolean;
  brightness: number;
  mode: Mode;
  hue: number;
  saturation: number;
  temperature: number;
  signal: number | null;
  effect: EffectName | null;
}

export interface Scene {
  name: string;
  builtin: boolean;
  brightness: number | null;
  temperature: number | null;
  color: HueSat | null;
}

export interface WebStatus {
  enabled: boolean;
  running: boolean;
  lan: boolean;
  hasPassword: boolean;
  localUrl: string | null;
  lanUrl: string | null;
  error: string | null;
}

export interface Snapshot {
  bulbs: Bulb[];
  scenes: Scene[];
  theme: Theme;
  web: WebStatus;
  hasAccount: boolean;
  temperatureRange: [number, number];
  backdrop: boolean;
  effectsAcknowledged: boolean;
}

export interface Patch {
  on?: boolean;
  brightness?: number;
  color?: HueSat;
  temperature?: number;
}

export interface Device {
  ip: string;
  name: string;
  room: string;
}

export interface SettingsView {
  devices: Device[];
  theme: Theme;
  desktop: boolean;
  username: string;
  hasTapoPassword: boolean;
  startMinimized: boolean;
  launchOnStartup: boolean;
  web: { enabled: boolean; port: number; lan: boolean; hasPassword: boolean };
}

export interface SettingsUpdate {
  devices: Device[];
  theme: Theme;
  username?: string;
  tapoPassword?: string;
  startMinimized?: boolean;
  launchOnStartup?: boolean;
  /** `password`: omitted/null keeps it, "" removes it, other text sets it. */
  web?: { enabled: boolean; port: number; lan: boolean; password?: string | null };
}

export interface SceneInput {
  brightness: number;
  color: HueSat | null;
  temperature: number | null;
}
