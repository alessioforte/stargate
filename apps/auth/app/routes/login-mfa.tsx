import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Box,
  Center,
  Button,
  Flex,
  PinInput,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import {
  clearPendingMfa,
  clearReturnTo,
  getPendingMfa,
  getReturnTo,
  hrefWithReturnTo,
  redirectToReturnTo,
  storePendingMfa,
  storeReturnTo,
} from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import type { LoginMFAResponse } from "@/services/types";
import useStore from "@/store";
import { useRouterLike } from "../../lib/navigation";

export default function LoginMFAPage() {
  const t = useTranslations();
  const router = useRouterLike();
  const [searchParams] = useSearchParams();
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);
  const pendingMfa = useStore((state) => state.pendingMfa);
  const verifyMFAChallenge = useStore((state) => state.verifyMFAChallenge);
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const [storedChallenge] = useState<LoginMFAResponse | null>(() =>
    getPendingMfa(),
  );
  const [localError, setLocalError] = useState<string | null>(null);
  const challenge = pendingMfa ?? storedChallenge;

  useEffect(() => {
    storeReturnTo(returnToParam);

    if (pendingMfa) {
      storePendingMfa(pendingMfa);
    }
  }, [pendingMfa, returnToParam]);

  const form = useForm({
    initialValues: {
      code: "",
    },
    validate: {
      code: (value) => (value.trim().length < 4 ? t("errors.code") : null),
    },
  });

  const handleSubmit = async (values: { code: string }) => {
    if (!challenge) {
      setLocalError(t("errors.missingChallenge"));
      return;
    }

    setLocalError(null);
    const result = await verifyMFAChallenge(
      challenge.challengeId,
      values.code.trim(),
    );

    if (result.status === "authenticated") {
      clearPendingMfa();
      clearReturnTo();
      redirectToReturnTo(router, returnTo);
    }
  };

  return (
    <Stack gap="lg">
      <Flex direction="column" align="center">
        <Title order={1} size="h2">
          {t("verificationCode")}
        </Title>
        <Text mt="md" c="dimmed" size="sm">
          {challenge?.method === "email" ? t("email") : "MFA"}
        </Text>
      </Flex>
      <form onSubmit={form.onSubmit(handleSubmit)}>
        <Stack gap="md">
          {(message || localError) && (
            <Alert
              color={message?.type === "error" || localError ? "red" : "yellow"}
              title={
                message?.type === "error" || localError ? t("error") : undefined
              }
            >
              {localError ?? message?.text}
            </Alert>
          )}
          <Flex my="md" justify="center">
            <PinInput
              length={6}
              oneTimeCode
              type="number"
              aria-label={t("verificationCode")}
              {...form.getInputProps("code")}
            />
          </Flex>
          <Button fullWidth type="submit" loading={loading}>
            {t("verify")}
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
