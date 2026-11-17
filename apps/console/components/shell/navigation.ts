import { getBasePath } from "@/lib/base-path";

export interface NavigationItem {
  icon: React.ReactNode;
  label: string;
  path: string;
}

export interface NavigationGroup {
  items: NavigationItem[];
  label?: string;
}

function normalizePath(path: string) {
  const normalized = path.replace(/\/+$/, "");
  return normalized || "/";
}

function pathWithoutBasePath(pathname: string) {
  const path = normalizePath(pathname);
  const basePath = getBasePath();

  if (basePath === "/" || !path.startsWith(basePath)) {
    return path;
  }

  if (path === basePath) {
    return "/";
  }

  return normalizePath(path.slice(basePath.length));
}

export function isActiveItem(pathname: string, itemPath: string) {
  const currentPath = pathWithoutBasePath(pathname);
  const targetPath = normalizePath(itemPath);

  if (targetPath === "/") {
    return currentPath === "/";
  }

  return currentPath === targetPath || currentPath.startsWith(`${targetPath}/`);
}

export function findActive(navigation: NavigationGroup[], pathname: string) {
  for (const group of navigation) {
    const item = group.items.find(({ path }) => isActiveItem(pathname, path));
    if (item) return { group, item };
  }

  return null;
}
