// TODO: write tests for all functions
type MutableRecord = Record<string, unknown>;

function isRecord(value: unknown): value is MutableRecord {
  return Boolean(value) && typeof value === "object";
}

class Accessor {
  static get(obj: unknown, path: string): unknown;
  static get<T>(obj: unknown, path: string, defaultValue: T): T | unknown;
  static get<T>(obj: unknown, path: string, defaultValue?: T): T | unknown {
    return path.split(".").reduce<unknown>((previous, current) => {
      if (!isRecord(previous)) return defaultValue;

      const value = previous[current];
      return value === undefined ? defaultValue : value;
    }, obj);
  }

  static set(obj: MutableRecord, path: string, value: unknown) {
    const segments = path.split(".");
    const key = segments.pop();
    let sub = obj;

    segments.forEach((segment) => {
      const current = sub[segment];

      if (!isRecord(current)) {
        sub[segment] = {};
      }

      sub = sub[segment] as MutableRecord;
    });

    if (key) {
      sub[key] = value;
    }
  }

  static delete(obj: MutableRecord, path: string) {
    const segments = path.split(".");
    const key = segments.pop();
    let sub = obj;

    segments.forEach((segment) => {
      const current = sub[segment];

      if (!isRecord(current)) {
        sub[segment] = {};
      }

      sub = sub[segment] as MutableRecord;
    });

    if (key) {
      delete sub[key];
    }
  }
}

export default Accessor;
