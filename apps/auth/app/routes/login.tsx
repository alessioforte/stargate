import { useEffect, type ReactNode } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Center,
  Button,
  Checkbox,
  Divider,
  Group,
  PasswordInput,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { FaGithub } from "react-icons/fa";
import { FcGoogle } from "react-icons/fc";
import {
  absoluteReturnTo,
  getReturnTo,
  hrefWithReturnTo,
  storeReturnTo,
} from "@/lib/auth-flow";
import { oauthLoginAction, socialSignInUrl } from "@/lib/env";
import { useHydrated } from "@/lib/hydration";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

/**
 * Codes sent back in `?error=`: by `/oauth/login` after a no-JS form post,
 * and by the social sign-in callbacks.
 */
const OAUTH_LOGIN_ERRORS = new Set([
  "invalid_credentials",
  "too_many_attempts",
  "interaction_expired",
  "javascript_required",
  "server_error",
  "social_cancelled",
  "social_email_unverified",
  "social_account_not_provisioned",
  "social_domain_not_allowed",
  "social_failed",
]);

const SOCIAL_PROVIDERS: Record<string, { name: string; icon: ReactNode }> = {
  google: { name: "Google", icon: <FcGoogle size={18} /> },
  github: { name: "GitHub", icon: <FaGithub size={18} /> },
};

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
  const features = useStore((state) => state.features);
  // This page is prerendered without a query string: read the URL only
  // once hydrated so the first render matches the static markup.
  const hydrated = useHydrated();
  const returnToParam = hydrated ? searchParams.get("return_to") : null;
  const returnTo = hydrated ? getReturnTo(searchParams) : null;
  const errorParam = hydrated ? searchParams.get("error") : null;
  const oauthError =
    errorParam && OAUTH_LOGIN_ERRORS.has(errorParam) ? errorParam : null;
  const passwordlessHref = hrefWithReturnTo("/login/passwordless", returnTo);
  const resetPasswordHref = hrefWithReturnTo("/reset-password", returnTo);
  const signupHref = hrefWithReturnTo("/signup", returnTo);
  const socialProviders = (features?.socialProviders ?? []).filter(
    (provider) => provider in SOCIAL_PROVIDERS,
  );

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

          {features?.passwordlessLogin && (
            <Anchor
              component={Link}
              to={passwordlessHref}
              ta="center"
              size="sm"
            >
              {t("passwordlessLogin")}
            </Anchor>
          )}

          <Button fullWidth type="submit" loading={loading}>
            {t("signin")}
          </Button>

          {features?.signupMode === "open" && (
            <Text ta="center" size="sm">
              {t("noAccount")}{" "}
              <Anchor component={Link} to={signupHref}>
                {t("signup")}
              </Anchor>
            </Text>
          )}
        </Stack>
      </form>
      {socialProviders.length > 0 && (
        <Stack gap="sm">
          <Divider label={t("or")} labelPosition="center" />
          {socialProviders.map((provider) => (
            <Button
              key={provider}
              component="a"
              href={socialSignInUrl(provider, absoluteReturnTo(returnTo))}
              variant="default"
              fullWidth
              leftSection={SOCIAL_PROVIDERS[provider].icon}
            >
              {t("continueWith", { provider: SOCIAL_PROVIDERS[provider].name })}
            </Button>
          ))}
        </Stack>
      )}
    </Stack>
  );
}
