import { useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Button,
  Center,
  Flex,
  Loader,
  PinInput,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

/**
 * OpenID Connect step-up. `/oauth/authorize` sends the browser here with
 * `?interaction=` when an application asks for multi-factor sign-in
 * (`acr_values`) and the session has one factor. Verifying an email code
 * adds the factor to the session and resumes the authorization request.
 */
export default function StepUp() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  const interactionId = searchParams.get("interaction");

  const challenge = useStore((state) => state.stepUpChallenge);
  const error = useStore((state) => state.stepUpError);
  const submitting = useStore((state) => state.stepUpSubmitting);
  const startStepUp = useStore((state) => state.startStepUp);
  const verifyStepUp = useStore((state) => state.verifyStepUp);

  // One code per visit, also under React's development double effects.
  const started = useRef(false);
  useEffect(() => {
    if (interactionId && !started.current) {
      started.current = true;
      void startStepUp();
    }
  }, [interactionId, startStepUp]);

  const form = useForm({
    initialValues: { code: "" },
    validate: {
      code: (value) => (value.trim().length < 4 ? t("errors.code") : null),
    },
  });

  const handleSubmit = async ({ code }: { code: string }) => {
    if (!interactionId) return;
    const result = await verifyStepUp(interactionId, code.trim());
    if (result.status === "success") {
      window.location.assign(result.redirectTo);
    }
  };

  if (!interactionId) {
    return (
      <Alert color="red" title={t("error")}>
        {t("stepUpMissing")}
      </Alert>
    );
  }

  return (
    <Stack gap="lg">
      <Flex direction="column" align="center">
        <Title order={1} size="h2">
          {t("stepUpTitle")}
        </Title>
        <Text mt="md" c="dimmed" size="sm" ta="center">
          {t("stepUpDescription")}
        </Text>
      </Flex>
      {error && (
        <Alert color="red" title={t("error")}>
          {error}
        </Alert>
      )}
      {!challenge && !error ? (
        <Center p="md">
          <Loader />
        </Center>
      ) : (
        <form onSubmit={form.onSubmit(handleSubmit)}>
          <Stack gap="md">
            <Flex my="md" justify="center">
              <PinInput
                length={6}
                oneTimeCode
                type="number"
                aria-label={t("verificationCode")}
                {...form.getInputProps("code")}
              />
            </Flex>
            <Button
              fullWidth
              type="submit"
              loading={submitting}
              disabled={!challenge}
            >
              {t("verify")}
            </Button>
            <Anchor
              component="button"
              type="button"
              ta="center"
              size="sm"
              onClick={() => void startStepUp()}
            >
              {t("stepUpResend")}
            </Anchor>
          </Stack>
        </form>
      )}
    </Stack>
  );
}
