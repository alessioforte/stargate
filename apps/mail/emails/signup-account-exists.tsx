import { Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailButton, EmailFallbackLink, EmailNote } from "./_components/kit";
import { leadStyle, paragraphLastStyle } from "./_components/theme";

interface SignupAccountExistsProps {
  loginUrl?: string;
  locale?: Locale;
}

export const SignupAccountExists = ({
  loginUrl = "{{link}}",
  locale = "en",
}: SignupAccountExistsProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "signup-account-exists.preview")}
      eyebrow={t(locale, "signup-account-exists.eyebrow")}
      title={t(locale, "signup-account-exists.title")}
    >
      <Text style={leadStyle}>{t(locale, "signup-account-exists.lead")}</Text>
      <Text style={paragraphLastStyle}>
        {t(locale, "signup-account-exists.body")}
      </Text>

      <EmailButton href={loginUrl}>
        {t(locale, "signup-account-exists.button")}
      </EmailButton>
      <EmailFallbackLink href={loginUrl} locale={locale} />

      <EmailNote title={t(locale, "signup-account-exists.noteTitle")}>
        {t(locale, "signup-account-exists.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default SignupAccountExists;
