import { useEffect } from "react";
import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Box,
  Button,
  Center,
  Loader,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { getReturnTo, hrefWithReturnTo, storeReturnTo } from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

/**
 * Starts a self-registration: the server emails a link that opens the
 * signup page with a token. Offered only when the server's signup mode is
 * `open`; otherwise accounts come from an administrator.
 */
export default function SignupRequest() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  const features = useStore((state) => state.features);
  const requestSignup = useStore((state) => state.requestSignup);
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const loginHref = hrefWithReturnTo("/login", returnTo);

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
    await requestSignup(values.email.trim());
  };

  if (!features) {
    return (
      <Center py="xl">
        <Loader size="sm" />
      </Center>
    );
  }

  if (features.signupMode !== "open") {
    return (
      <Stack gap="lg">
        <Box>
          <Center mb="lg">
            <Title order={1} size="h2">
              {t("signupUnavailableTitle")}
            </Title>
          </Center>
          <Text c="dimmed" size="sm">
            {t("signupUnavailableDescription")}
          </Text>
        </Box>
        <Button component={Link} to={loginHref} fullWidth>
          {t("backToLogin")}
        </Button>
      </Stack>
    );
  }

  return (
    <Stack gap="lg">
      <Box>
        <Center mb="lg">
          <Title order={1} size="h2">
            {t("signupRequestTitle")}
          </Title>
        </Center>
        <Text c="dimmed" size="sm">
          {t("signupRequestDescription")}
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
            {t("signupRequestSubmit")}
          </Button>
          <Anchor mt="md" component={Link} to={loginHref} ta="center" size="sm">
            {t("backToLogin")}
          </Anchor>
        </Stack>
      </form>
    </Stack>
  );
}
