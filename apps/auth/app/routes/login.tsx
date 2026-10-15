import { useEffect } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Center,
  Button,
  Checkbox,
  Group,
  PasswordInput,
  Stack,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { getReturnTo, hrefWithReturnTo, storeReturnTo } from "@/lib/auth-flow";
import { oauthLoginAction } from "@/lib/env";
import { useHydrated } from "@/lib/hydration";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

/** Codes `/oauth/login` sends back in `?error=` after a no-JS form post. */
const OAUTH_LOGIN_ERRORS = new Set([
  "invalid_credentials",
  "too_many_attempts",
  "interaction_expired",
  "javascript_required",
  "server_error",
]);

interface LoginValues {
  username: string;
  password: string;
  remember: boolean;
}

export default function Login() {
  const t = useTranslations();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const loading = useStore((state) => state.loading);
  const login = useStore((state) => state.login);
  const message = useStore((state) => state.message);
  // This page is prerendered without a query string: read the URL only
  // once hydrated so the first render matches the static markup.
  const hydrated = useHydrated();
  const returnToParam = hydrated ? searchParams.get("return_to") : null;
  const returnTo = hydrated ? getReturnTo(searchParams) : null;
  const errorParam = hydrated ? searchParams.get("error") : null;
  const oauthError =
    errorParam && OAUTH_LOGIN_ERRORS.has(errorParam) ? errorParam : null;
  // const passwordlessHref = hrefWithReturnTo("/login/passwordless", returnTo);
  const resetPasswordHref = hrefWithReturnTo("/reset-password", returnTo);

  useEffect(() => {
    storeReturnTo(returnToParam);
  }, [returnToParam]);

  const form = useForm<LoginValues>({
    initialValues: {
      username: "",
      password: "",
      remember: false,
    },
    validate: {
      username: (value) =>
        value.trim().length < 3 ? "Username must be at least 3 chars" : null,
      password: (value) =>
        value.length > 5 ? null : "Password must be at least 6 characters",
    },
  });

  // OpenID Connect `login_hint`, forwarded by /oauth/authorize.
  const loginHint = hydrated ? searchParams.get("login_hint") : null;
  useEffect(() => {
    if (loginHint && !form.values.username) {
      form.setFieldValue("username", loginHint);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loginHint]);

  const handleSubmit = async ({ username, password }: LoginValues) => {
    const result = await login({
      username: username.trim(),
      password,
    });

    if (result.status === "authenticated") {
      // The picker redirects straight to return_to unless the user belongs
      // to more than one organization.
      navigate(hrefWithReturnTo("/select-organization", returnTo));
      return;
    }

    if (result.status === "mfa_required") {
      navigate(hrefWithReturnTo("/login/mfa", returnTo));
    }
  };

  return (
    <Stack gap="lg">
      <Center>
        <Title order={1} size="h2">
          {t("loginTitle")}
        </Title>
      </Center>
      {/* With JavaScript the submit is handled in-page. Without it (or before
          the bundle loads) the browser posts the form to the provider login
          endpoint, which resumes a parked OpenID Connect authorization. */}
      <form
        method="post"
        action={oauthLoginAction()}
        onSubmit={form.onSubmit(handleSubmit)}
      >
        <Stack gap="md">
          {oauthError && !message && (
            <Alert color="red" title={t("error")}>
              {t(`oauthLoginErrors.${oauthError}`)}
            </Alert>
          )}
          {message && (
            <Alert
              color={message.type === "error" ? "red" : "yellow"}
              title={message.type === "error" ? t("error") : undefined}
            >
              {message.text}
            </Alert>
          )}
          <TextInput
            withAsterisk
            style={{ minHeight: 90 }}
            type="text"
            variant="filled"
            label={t("username")}
            placeholder={t("username")}
            autoComplete="username"
            name="username"
            {...form.getInputProps("username")}
          />
          <PasswordInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("password")}
            placeholder={t("password")}
            autoComplete="current-password"
            name="password"
            {...form.getInputProps("password")}
          />
          <Group mb="md" justify="space-between" align="center">
            <Checkbox
              label={t("rememberMe")}
              {...form.getInputProps("remember", { type: "checkbox" })}
            />
            <Anchor component={Link} to={resetPasswordHref} size="sm">
              {t("forgotPassword")}
            </Anchor>
          </Group>

          {/*<Anchor component={Link} to={passwordlessHref} ta="center" size="sm">
            {t("passwordlessLogin")}
          </Anchor>*/}

          <Button fullWidth type="submit" loading={loading}>
            {t("signin")}
          </Button>
        </Stack>
      </form>
    </Stack>
  );
}
