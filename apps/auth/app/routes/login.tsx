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
import {
  clearReturnTo,
  getReturnTo,
  hrefWithReturnTo,
  redirectToReturnTo,
  storeReturnTo,
} from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import { useRouterLike } from "../../lib/navigation";

interface LoginValues {
  username: string;
  password: string;
  remember: boolean;
}

export default function Login() {
  const t = useTranslations();
  const router = useRouterLike();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const loading = useStore((state) => state.loading);
  const login = useStore((state) => state.login);
  const message = useStore((state) => state.message);
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const passwordlessHref = hrefWithReturnTo("/login/passwordless", returnTo);
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

  const handleSubmit = async ({ username, password }: LoginValues) => {
    const result = await login({
      username: username.trim(),
      password,
    });

    if (result.status === "authenticated") {
      clearReturnTo();
      redirectToReturnTo(router, returnTo);
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
      <form onSubmit={form.onSubmit(handleSubmit)}>
        <Stack gap="md">
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
            {...form.getInputProps("username")}
          />
          <PasswordInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("password")}
            placeholder={t("password")}
            autoComplete="current-password"
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
