import { useEffect, useRef, useState } from "react";
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
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { getReturnTo, hrefWithReturnTo, storeReturnTo } from "@/lib/auth-flow";
import { useTranslations } from "@/i18n";
import type { SignupVerificationResponse } from "@/services/types";
import useStore from "@/store";

interface SignupValues {
  email: string;
  givenName: string;
  familyName: string;
  nickname: string;
  phoneNumber: string;
  password: string;
  confirmPassword: string;
}

const emptyValues: SignupValues = {
  email: "",
  givenName: "",
  familyName: "",
  nickname: "",
  phoneNumber: "",
  password: "",
  confirmPassword: "",
};

function signupCacheKey(token: string) {
  return `stargate.auth.signup.${token}`;
}

function readCachedSignup(token: string): SignupVerificationResponse | null {
  const raw = window.sessionStorage.getItem(signupCacheKey(token));
  if (!raw) return null;

  try {
    return JSON.parse(raw) as SignupVerificationResponse;
  } catch {
    window.sessionStorage.removeItem(signupCacheKey(token));
    return null;
  }
}

function cacheSignup(sourceToken: string, signup: SignupVerificationResponse) {
  window.sessionStorage.setItem(
    signupCacheKey(sourceToken),
    JSON.stringify(signup),
  );
}

function clearCachedSignup(sourceToken: string) {
  window.sessionStorage.removeItem(signupCacheKey(sourceToken));
}

export default function SignupPage() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();

  const loadSignup = useStore((state) => state.loadSignup);
  const completeSignup = useStore((state) => state.completeSignup);
  const loading = useStore((state) => state.loading);
  const message = useStore((state) => state.message);

  const returnToParam = searchParams.get("return_to");
  const returnTo = getReturnTo(searchParams);
  const sourceToken = searchParams.get("token") ?? "";

  const [completionToken, setCompletionToken] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [completed, setCompleted] = useState(false);

  const form = useForm<SignupValues>({
    initialValues: emptyValues,
    validate: {
      email: (value) => (!value.includes("@") ? t("errors.email") : null),
      givenName: (value) =>
        value.trim() ? null : t("errors.givenNameRequired"),
      familyName: (value) =>
        value.trim() ? null : t("errors.familyNameRequired"),
      nickname: (value) => (value.trim() ? null : t("errors.nicknameRequired")),
      password: (value) =>
        value.length > 5 ? null : t("errors.passwordTooShort"),
      confirmPassword: (value, values) =>
        value === values.password ? null : t("errors.passwordMismatch"),
    },
  });
  const formRef = useRef(form);
  formRef.current = form;

  useEffect(() => {
    storeReturnTo(returnToParam);
  }, [returnToParam]);

  useEffect(() => {
    if (!sourceToken) {
      setLocalError(t("errors.invalidSignupToken"));
      return;
    }

    const applySignup = (signup: SignupVerificationResponse) => {
      setCompletionToken(signup.token);
      formRef.current.setValues({
        email: signup.email,
        givenName: signup.givenName ?? "",
        familyName: signup.familyName ?? "",
        nickname: signup.nickname ?? "",
        phoneNumber: signup.phoneNumber ?? "",
        password: "",
        confirmPassword: "",
      });
      setLocalError(null);
    };

    const cachedSignup = readCachedSignup(sourceToken);
    if (cachedSignup) {
      applySignup(cachedSignup);
      return;
    }

    let cancelled = false;

    void loadSignup(sourceToken).then((result) => {
      if (cancelled) return;

      if (result.status === "error") {
        setLocalError(result.message);
        return;
      }

      cacheSignup(sourceToken, result.signup);
      applySignup(result.signup);
    });

    return () => {
      cancelled = true;
    };
  }, [sourceToken, loadSignup, t]);

  const handleSubmit = async (values: SignupValues) => {
    if (!completionToken || !sourceToken) {
      setLocalError(t("errors.invalidSignupToken"));
      return;
    }

    setLocalError(null);
    const result = await completeSignup({
      token: completionToken,
      givenName: values.givenName.trim(),
      familyName: values.familyName.trim(),
      nickname: values.nickname.trim(),
      password: values.password,
      phoneNumber: values.phoneNumber.trim() || null,
    });

    if (result.status === "success") {
      clearCachedSignup(sourceToken);
      setCompleted(true);
    }
  };

  if (completed) {
    return (
      <Stack gap="lg">
        <Box>
          <Center mb="lg">
            <Title order={1} size="h2">
              {t("accountCreatedMessage")}
            </Title>
          </Center>
          <Text c="dimmed" size="sm">
            {message?.text ?? t("signupCompletedDescription")}
          </Text>
        </Box>
        <Button
          component={Link}
          to={hrefWithReturnTo("/login", returnTo)}
          fullWidth
        >
          {t("goToLogin")}
        </Button>
      </Stack>
    );
  }

  return (
    <Stack my={60} gap="lg">
      <Box>
        <Center mb="lg">
          <Title order={1} size="h2">
            {t("signupTitle")}
          </Title>
        </Center>
        <Text c="dimmed" size="sm">
          {t("signupDescription")}
        </Text>
      </Box>
      <form onSubmit={form.onSubmit(handleSubmit)}>
        <Stack gap={0}>
          {(message || localError) && (
            <Alert
              mb="lg"
              color={message?.type === "error" || localError ? "red" : "green"}
              title={
                message?.type === "error" || localError ? t("error") : undefined
              }
            >
              {localError ?? message?.text}
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
            readOnly
            {...form.getInputProps("email")}
          />
          <TextInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("firstName")}
            placeholder={t("firstName")}
            autoComplete="given-name"
            {...form.getInputProps("givenName")}
          />
          <TextInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("lastName")}
            placeholder={t("lastName")}
            autoComplete="family-name"
            {...form.getInputProps("familyName")}
          />
          <TextInput
            withAsterisk
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("username")}
            placeholder={t("username")}
            autoComplete="username"
            {...form.getInputProps("nickname")}
          />
          <TextInput
            style={{ minHeight: 90 }}
            variant="filled"
            label={t("phoneNumber")}
            placeholder={t("phoneNumber")}
            autoComplete="tel"
            {...form.getInputProps("phoneNumber")}
          />
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
            {t("createAccount")}
          </Button>
          <Anchor
            mt="lg"
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
