type SettingsData = Record<string, unknown>;

function parseSettings(data: string | null): SettingsData {
  if (!data) {
    return {};
  }

  try {
    const parsed: unknown = JSON.parse(data);

    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as SettingsData;
    }
  } catch {
    return {};
  }

  return {};
}

export default class Settings {
  static set(key: string, value: unknown) {
    if (typeof window === "undefined") return;

    const settings = parseSettings(localStorage.getItem("settings"));
    settings[key] = value;
    localStorage.setItem("settings", JSON.stringify(settings));
  }

  static get(): SettingsData | null;
  static get<T = unknown>(key: string): T | null;
  static get<T>(key: string, defaultValue: T): T;
  static get<T = unknown>(
    key?: string,
    defaultValue: T | null = null,
  ): T | SettingsData | null {
    if (typeof window === "undefined") return defaultValue;

    const settings = parseSettings(localStorage.getItem("settings"));

    if (!key) {
      return Object.keys(settings).length > 0 ? settings : defaultValue;
    }

    const value = settings[key];
    return value === undefined ? defaultValue : (value as T);
  }
}
