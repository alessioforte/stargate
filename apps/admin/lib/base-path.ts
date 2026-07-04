function normalizeBasePath(value: string | undefined) {
  const trimmed = value?.trim().replace(/^\/+|\/+$/g, "");
  return trimmed ? `/${trimmed}` : "/";
}

export function getBasePath() {
  return normalizeBasePath(import.meta.env.BASE_URL);
}

export function appPath(path: string) {
  const basePath = getBasePath();
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;

  if (basePath === "/") {
    return normalizedPath;
  }

  return `${basePath}${normalizedPath === "/" ? "" : normalizedPath}`;
}
