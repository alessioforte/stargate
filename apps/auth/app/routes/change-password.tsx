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
import type { FlowContinuation } from "@/services/types";
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
  const [completed, setCompleted] = useState(false);
  const [continuation, setContinuation] = useState<FlowContinuation | null>(
    null,
  );

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
    const result = await changePassword(values.password, token);
    if (result.status === "success") {
      setContinuation(result.continuation ?? null);
      setCompleted(true);
    }
  };

  if (completed) {
    return (
      <Stack gap="lg">
        <Box>
          <Center mb="lg">
            <Title order={1} size="h2">
              {t("passwordChangedMessage")}
            </Title>
          </Center>
          {message && (
            <Text c="dimmed" size="sm">
              {message.text}
            </Text>
          )}
        </Box>
        {continuation ? (
          <Button component="a" href={continuation.continueUrl} fullWidth>
            {t("continueToApp", { app: continuation.clientName })}
          </Button>
        ) : (
          <Button
            component={Link}
            to={hrefWithReturnTo("/login", returnTo)}
            fullWidth
          >
            {t("goToLogin")}
          </Button>
        )}
      </Stack>
    );
  }

  return (
    <Stack gap="lg">
      <Box>
        <Center>
          <Title order={1} size="h2">
            {t("changePassword")}
          </Title>
        </Center>
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
