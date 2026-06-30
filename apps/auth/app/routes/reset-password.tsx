import { useEffect } from "react";
import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Box,
  Button,
  Center,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { getReturnTo, hrefWithReturnTo, storeReturnTo } from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

export default function ResetPasswordPage() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  const forgotPassword = useStore((state) => state.forgotPassword);
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);

  useEffect(() => {
    storeReturnTo(returnToParam);
  }, [returnToParam]);

  const form = useForm({
    initialValues: {
      email: "",
    },
    validate: {
      email: (value) => (!value.includes("@") ? t("errors.email") : null),
    },
  });

  const handleSubmit = async (values: { email: string }) => {
    await forgotPassword(values.email.trim());
  };

  return (
    <Stack gap="lg">
      <Box>
        <Center mb="lg">
          <Title order={1} size="h2">
            {t("forgotPasswordTitle")}
          </Title>
        </Center>
        <Text c="dimmed" size="sm">
          {t("recoverPasswordDescription")}
        </Text>
      </Box>
      <form onSubmit={form.onSubmit(handleSubmit)}>
        <Stack gap="md">
          {message && (
            <Alert
              color={message.type === "error" ? "red" : "green"}
              title={message.type === "error" ? t("error") : undefined}
            >
              {message.text}
            </Alert>
          )}
          <TextInput
            withAsterisk
            style={{ minHeight: 90 }}
            type="email"
            variant="filled"
            label={t("email")}
            placeholder={t("email")}
            autoComplete="email"
            {...form.getInputProps("email")}
          />
          <Button fullWidth type="submit" loading={loading}>
            {t("resetPassword")}
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
