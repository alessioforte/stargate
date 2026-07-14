import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import {
  Alert,
  Badge,
  Center,
  Group,
  Loader,
  Stack,
  Text,
  Title,
  UnstyledButton,
} from "@mantine/core";
import {
  clearReturnTo,
  getReturnTo,
  redirectToReturnTo,
} from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import services from "@/services";
import type { AccountOrganization } from "@/services/types";
import { useRouterLike } from "../../lib/navigation";
import classes from "./select-organization.module.css";

/**
 * Post-login org picker. The login flow lands here after a session is
 * issued: with zero or one membership the page redirects straight to
 * `return_to`; with more it lets the user pick the org the session acts
 * in. Picking the already-active org just continues; any other org mints
 * a fresh session (and cookie) via the switch endpoint.
 */
export default function SelectOrganization() {
  const t = useTranslations();
  const router = useRouterLike();
  const [searchParams] = useSearchParams();
  const returnTo = getReturnTo(searchParams);

  const [organizations, setOrganizations] = useState<
    AccountOrganization[] | null
  >(null);
  const [error, setError] = useState<string | null>(null);
  const [switching, setSwitching] = useState<string | null>(null);
  const startedRef = useRef(false);

  const finishLogin = () => {
    clearReturnTo();
    redirectToReturnTo(router, returnTo);
  };

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;

    services.auth.getOrganizations().then((res) => {
      if (res.error || !res.data) {
        // No valid session (deep link or expired login): back to login.
        router.replace("/login");
        return;
      }
      if (res.data.organizations.length <= 1) {
        finishLogin();
        return;
      }
      setOrganizations(res.data.organizations);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSelect = async (org: AccountOrganization) => {
    if (switching) return;
    setError(null);

    if (org.active) {
      finishLogin();
      return;
    }

    setSwitching(org.id);
    const res = await services.auth.switchOrganization(org.id);
    setSwitching(null);

    if (res.error) {
      setError(res.message ?? t("error"));
      return;
    }
    finishLogin();
  };

  return (
    <Stack gap="lg">
      <Center>
        <Title order={1} size="h2">
          {t("chooseOrganization")}
        </Title>
      </Center>

      {error && (
        <Alert color="red" title={t("error")}>
          {error}
        </Alert>
      )}

      {organizations === null ? (
        <Center p="xl">
          <Loader />
        </Center>
      ) : (
        <Stack gap="sm">
          <Text c="dimmed" size="sm" ta="center">
            {t("chooseOrganizationHint")}
          </Text>
          {organizations.map((org) => (
            <UnstyledButton
              key={org.id}
              className={classes.orgButton}
              disabled={Boolean(switching)}
              data-active={org.active || undefined}
              onClick={() => handleSelect(org)}
            >
              <Group justify="space-between" wrap="nowrap">
                <Stack gap={0} style={{ minWidth: 0 }}>
                  <Text fw={600} truncate>
                    {org.name}
                  </Text>
                  <Text size="xs" c="dimmed" tt="capitalize">
                    {org.role}
                  </Text>
                </Stack>
                {switching === org.id ? (
                  <Loader size="xs" />
                ) : (
                  org.active && (
                    <Badge variant="light" size="sm">
                      {t("currentOrganization")}
                    </Badge>
                  )
                )}
              </Group>
            </UnstyledButton>
          ))}
        </Stack>
      )}
    </Stack>
  );
}
