import { useEffect } from "react";
import { useSearchParams } from "react-router";
import {
  Alert,
  Anchor,
  Avatar,
  Button,
  Center,
  Group,
  List,
  Loader,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useTranslations } from "@/i18n";
import useStore from "@/store";

/**
 * OAuth/OIDC consent page. `/oauth/authorize` sends the browser here with
 * `?interaction=` when the signed-in user has not yet allowed the
 * application; the decision resumes or ends that authorization request.
 */
export default function Consent() {
  const t = useTranslations();
  const [searchParams] = useSearchParams();
  const interactionId = searchParams.get("interaction");

  const prompt = useStore((state) => state.consentPrompt);
  const error = useStore((state) => state.consentError);
  const submitting = useStore((state) => state.consentSubmitting);
  const loadConsentPrompt = useStore((state) => state.loadConsentPrompt);
  const decideConsent = useStore((state) => state.decideConsent);

  useEffect(() => {
    if (interactionId) {
      void loadConsentPrompt(interactionId);
    }
  }, [interactionId, loadConsentPrompt]);

  const decide = async (approve: boolean) => {
    if (!interactionId) return;
    const result = await decideConsent(interactionId, approve);
    if (result.status === "success") {
      window.location.assign(result.redirectTo);
    }
  };

  // Unknown scopes (an API's own scopes) are shown as they are.
  const scopeLabel = (scope: string) => {
    const key = `consentScopes.${scope}`;
    const label = t(key);
    return label === key ? scope : label;
  };

  if (!interactionId) {
    return (
      <Alert color="red" title={t("error")}>
        {t("consentMissing")}
      </Alert>
    );
  }

  if (!prompt) {
    return error ? (
      <Alert color="red" title={t("error")}>
        {error}
      </Alert>
    ) : (
      <Center p="xl">
        <Loader />
      </Center>
    );
  }

  const { client } = prompt;
  const links = [
    { href: client.clientUri, label: t("consentWebsite") },
    { href: client.policyUri, label: t("consentPolicy") },
    { href: client.tosUri, label: t("consentTerms") },
  ].filter((link): link is { href: string; label: string } =>
    Boolean(link.href),
  );

  return (
    <Stack gap="lg">
      <Stack gap="xs" align="center">
        {client.logoUri && (
          <Avatar src={client.logoUri} alt={client.name} size="lg" />
        )}
        <Title order={1} size="h3" ta="center">
          {t("consentTitle", { client: client.name })}
        </Title>
        <Text size="sm" c="dimmed" ta="center">
          {t("consentSignedInAs", { email: prompt.userEmail })}
        </Text>
      </Stack>

      {error && (
        <Alert color="red" title={t("error")}>
          {error}
        </Alert>
      )}

      <Stack gap="xs">
        <Text fw={600}>{t("consentRequests")}</Text>
        <List spacing="xs">
          {prompt.scopes.map((scope) => (
            <List.Item key={scope}>{scopeLabel(scope)}</List.Item>
          ))}
          {prompt.audience && (
            <List.Item>
              {t("consentAudience", { audience: prompt.audience })}
            </List.Item>
          )}
        </List>
      </Stack>

      {client.redirectHost && (
        <Text size="sm" c="dimmed">
          {t("consentRedirectHost", { host: client.redirectHost })}
        </Text>
      )}

      {links.length > 0 && (
        <Group gap="md">
          {links.map((link) => (
            <Anchor
              key={link.label}
              href={link.href}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
            >
              {link.label}
            </Anchor>
          ))}
        </Group>
      )}

      <Group grow>
        <Button
          variant="default"
          disabled={submitting}
          onClick={() => void decide(false)}
        >
          {t("consentDeny")}
        </Button>
        <Button loading={submitting} onClick={() => void decide(true)}>
          {t("consentAllow")}
        </Button>
      </Group>
    </Stack>
  );
}
