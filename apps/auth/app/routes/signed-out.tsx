import { Link } from "react-router";
import { Button, Center, Stack, Text, Title } from "@mantine/core";
import { useTranslations } from "@/i18n";

/** Where `/oauth/logout` ends when the application gave no return address. */
export default function SignedOut() {
  const t = useTranslations();

  return (
    <Stack gap="lg">
      <Center>
        <Title order={1} size="h2">
          {t("signedOutTitle")}
        </Title>
      </Center>
      <Text ta="center" c="dimmed">
        {t("signedOutDescription")}
      </Text>
      <Button component={Link} to="/login" variant="light">
        {t("signInAgain")}
      </Button>
    </Stack>
  );
}
