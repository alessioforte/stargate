import { useMemo } from "react";
import { type NavigateFunction, useNavigate } from "react-router";
import type { RouterLike } from "@/lib/auth-flow";

export function useRouterLike(): RouterLike {
  const navigate = useNavigate();

  return useMemo(
    () => ({
      replace: (href: string) => navigate(href, { replace: true }),
    }),
    [navigate],
  );
}

export function push(navigate: NavigateFunction, href: string) {
  navigate(href);
}
