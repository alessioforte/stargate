function normalizeBasePath(value: string | undefined) {
  const trimmed = value?.trim().replace(/^\/+|\/+$/g, "");
  return trimmed ? `/${trimmed}` : "/";
}

export function getAuthBasePath() {
  return normalizeBasePath(import.meta.env.BASE_URL);
}
