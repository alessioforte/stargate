import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Button,
  Center,
  Group,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { oauthLogoutConfirmAction } from "@/lib/env";
import { useHydrated } from "@/lib/hydration";
import { useTranslations } from "@/i18n";

/** Codes `/oauth/logout/confirm` sends back in `?error=`. */
const LOGOUT_ERRORS = new Set(["logout_expired"]);

/**
 * OpenID Connect logout confirmation. `/oauth/logout` sends the browser here
 * when an application asks to sign the user out without proving it is the
 * user's own session. The page is prerendered and the form posts natively,
 * so it works without JavaScript; the parked request travels in a cookie.
 */
export default function Logout() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  // Prerendered without a query string: read the URL once hydrated.
  const hydrated = useHydrated();
  const errorParam = hydrated ? searchParams.get("error") : null;
  const error = errorParam && LOGOUT_ERRORS.has(errorParam) ? errorParam : null;

  if (error) {
    return (
      <Stack gap="lg">
        <Alert color="red" title={t("error")}>
          {t(`logoutErrors.${error}`)}
        </Alert>
        <Center>
          <Anchor component={Link} to="/login">
            {t("signInAgain")}
          </Anchor>
        </Center>
      </Stack>
    );
  }

  return (
    <Stack gap="lg">
      <Center>
        <Title order={1} size="h2">
          {t("logoutTitle")}
        </Title>
      </Center>
      <Text ta="center" c="dimmed">
        {t("logoutDescription")}
      </Text>
      <form method="post" action={oauthLogoutConfirmAction()}>
        <Group grow>
          <Button type="submit" name="action" value="cancel" variant="default">
            {t("logoutCancel")}
          </Button>
          <Button type="submit" name="action" value="logout" color="red">
            {t("logoutConfirm")}
          </Button>
        </Group>
      </form>
    </Stack>
  );
}
