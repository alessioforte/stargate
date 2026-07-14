import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
  Alert,
  Box,
  Center,
  Button,
  PinInput,
  Stack,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import {
  clearPendingPasswordless,
  getPendingPasswordless,
  getReturnTo,
  hrefWithReturnTo,
  storePendingPasswordless,
  storeReturnTo,
  type PendingPasswordlessChallenge,
} from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import { useRouterLike } from "../../lib/navigation";

export default function PasswordlessLoginPage() {
  const t = useTranslations();
  const router = useRouterLike();
  const [searchParams] = useSearchParams();
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);
  const pendingPasswordless = useStore((state) => state.pendingPasswordless);
  const requestPasswordlessLogin = useStore(
    (state) => state.requestPasswordlessLogin,
  );
  const verifyPasswordlessLogin = useStore(
    (state) => state.verifyPasswordlessLogin,
  );
  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const [localChallenge, setLocalChallenge] =
    useState<PendingPasswordlessChallenge | null>(() =>
      getPendingPasswordless(),
    );
  const [localError, setLocalError] = useState<string | null>(null);
  const challenge = pendingPasswordless ?? localChallenge;

  useEffect(() => {
    storeReturnTo(returnToParam);
  }, [returnToParam]);

  const emailForm = useForm({
    initialValues: {
      email: "",
    },
    validate: {
      email: (value) => (!value.includes("@") ? t("errors.email") : null),
    },
  });

  const codeForm = useForm({
    initialValues: {
      code: "",
    },
    validate: {
      code: (value) => (value.trim().length < 4 ? t("errors.code") : null),
    },
  });

  const handleEmailSubmit = async (values: { email: string }) => {
    setLocalError(null);
    const email = values.email.trim();
    const result = await requestPasswordlessLogin(email);

    if (result.status === "challenge_sent") {
      const nextChallenge = {
        ...result.challenge,
        email,
      };
      setLocalChallenge(nextChallenge);
      storePendingPasswordless(nextChallenge);
    }
  };

  const handleCodeSubmit = async (values: { code: string }) => {
    if (!challenge) {
      setLocalError(t("errors.missingChallenge"));
      return;
    }

    setLocalError(null);
    const result = await verifyPasswordlessLogin(
      challenge.challengeId,
      values.code.trim(),
    );

    if (result.status === "authenticated") {
      clearPendingPasswordless();
      router.replace(hrefWithReturnTo("/select-organization", returnTo));
    }
  };

  return (
    <Stack gap="lg">
      <Center>
        <Title order={1} size="h2">
          {t("passwordlessLogin")}
        </Title>
        {/*<Text c="dimmed" size="sm">
          {challenge ? challenge.email : t("email")}
        </Text>*/}
      </Center>
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
      {!challenge ? (
        <form onSubmit={emailForm.onSubmit(handleEmailSubmit)}>
          <Stack gap="md">
            <TextInput
              withAsterisk
              style={{ minHeight: 90 }}
              type="email"
              variant="filled"
              label={t("email")}
              placeholder={t("email")}
              autoComplete="email"
              {...emailForm.getInputProps("email")}
            />
            <Button fullWidth type="submit" loading={loading}>
              {t("sendCode")}
            </Button>
          </Stack>
        </form>
      ) : (
        <form onSubmit={codeForm.onSubmit(handleCodeSubmit)}>
          <Stack gap="md">
            <Box mx="auto">
              <PinInput
                length={6}
                oneTimeCode
                type="number"
                aria-label={t("verificationCode")}
                {...codeForm.getInputProps("code")}
              />
            </Box>
            <Button fullWidth type="submit" loading={loading}>
              {t("verify")}
            </Button>
          </Stack>
        </form>
      )}
      {/*<Anchor
        mt="md"
        component={Link}
        to={hrefWithReturnTo("/login", returnTo)}
        ta="center"
        size="sm"
      >
        {t("backToLogin")}
      </Anchor>*/}
    </Stack>
  );
}
