import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Box,
  Button,
  Center,
  PasswordInput,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { getReturnTo, hrefWithReturnTo, storeReturnTo } from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

interface ChangePasswordValues {
  password: string;
  confirmPassword: string;
}

export default function ChangePasswordPage() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  const changePassword = useStore((state) => state.changePassword);
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const token = searchParams.get("token") ?? "";
  const [localError, setLocalError] = useState<string | null>(null);

  useEffect(() => {
    storeReturnTo(returnToParam);
  }, [returnToParam]);

  const form = useForm<ChangePasswordValues>({
    initialValues: {
      password: "",
      confirmPassword: "",
    },
    validate: {
      password: (value) =>
        value.length > 5 ? null : t("errors.passwordTooShort"),
      confirmPassword: (value, values) =>
        value === values.password ? null : t("errors.passwordMismatch"),
    },
  });

  const handleSubmit = async (values: ChangePasswordValues) => {
    if (!token) {
      setLocalError(t("errors.invalidToken"));
      return;
    }

    setLocalError(null);
    await changePassword(values.password, token);
  };

  return (
    <Stack gap="lg">
      <Box>
        <Center>
          <Title order={1} size="h2">
            {t("changePassword")}
          </Title>
        </Center>
        {/*<Text c="dimmed" size="sm">
          Stargate
        </Text>*/}
      </Box>
      <form onSubmit={form.onSubmit(handleSubmit)}>
        <Stack gap="md">
          {(message || localError) && (
            <Alert
              color={message?.type === "error" || localError ? "red" : "green"}
              title={
                message?.type === "error" || localError ? t("error") : undefined
              }
            >
              {localError ?? message?.text}
            </Alert>
          )}
          <PasswordInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("newPassword")}
            placeholder={t("newPassword")}
            autoComplete="new-password"
            {...form.getInputProps("password")}
          />
          <PasswordInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("confirmPassword")}
            placeholder={t("confirmPassword")}
            autoComplete="new-password"
            {...form.getInputProps("confirmPassword")}
          />
          <Button mt="md" fullWidth type="submit" loading={loading}>
            {t("changePassword")}
          </Button>
          <Anchor
            mt="md"
            component={Link}
            to={hrefWithReturnTo("/login", returnTo)}
            ta="center"
            size="sm"
          >
            {t("backToLogin")}
          </Anchor>
        </Stack>
      </form>
    </Stack>
  );
}
