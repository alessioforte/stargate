import type { JsonValue } from "@/services/types";

export interface GatewayNamedFormProps {
  canDelete?: boolean;
  existingNames: string[];
  selectedName: string;
  selectedValue: JsonValue | null;
  onApply: (previousName: string, nextName: string, value: JsonValue) => void;
  onDelete: (name: string) => void;
}

export function getObjectNameErrorKey(
  name: string,
  selectedName: string,
  existingNames: string[],
) {
  if (!name) return "objectNameRequired";
  if (name !== selectedName && existingNames.includes(name)) {
    return "duplicateObjectName";
  }

  return null;
}

export function positiveInteger(
  value: number | string,
  max = Number.MAX_SAFE_INTEGER,
) {
  const numberValue = Number(value);
  return Number.isInteger(numberValue) && numberValue > 0 && numberValue <= max
    ? numberValue
    : null;
}

export function optionalPositiveInteger(
  value: number | string,
  max = Number.MAX_SAFE_INTEGER,
) {
  if (value === "") return undefined;
  return positiveInteger(value, max);
}

export function trimmedOrUndefined(value: string) {
  const trimmed = value.trim();
  return trimmed || undefined;
}

export function nameOptions(names: string[]) {
  return names
    .filter((name) => name.trim())
    .map((name) => ({ value: name, label: name }));
}
