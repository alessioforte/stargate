import { useSyncExternalStore } from "react";

const subscribe = () => () => {};

/**
 * False while React hydrates prerendered markup, true afterwards (and from
 * the start when the page is rendered on the client only). Gate anything
 * read from the URL or browser storage behind it so the first client render
 * matches the prerendered HTML.
 */
export function useHydrated() {
  return useSyncExternalStore(
    subscribe,
    () => true,
    () => false,
  );
}
