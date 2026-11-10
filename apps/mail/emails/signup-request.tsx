import { Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailButton, EmailFallbackLink, EmailNote } from "./_components/kit";
import { leadStyle, paragraphLastStyle } from "./_components/theme";

interface SignupRequestProps {
  signupUrl?: string;
  locale?: Locale;
}

export const SignupRequest = ({
  signupUrl = "{{link}}",
  locale = "en",
}: SignupRequestProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "signup-request.preview")}
      eyebrow={t(locale, "signup-request.eyebrow")}
      title={t(locale, "signup-request.title")}
    >
      <Text style={leadStyle}>{t(locale, "signup-request.lead")}</Text>
      <Text style={paragraphLastStyle}>{t(locale, "signup-request.body")}</Text>

      <EmailButton href={signupUrl}>
        {t(locale, "signup-request.button")}
      </EmailButton>
      <EmailFallbackLink href={signupUrl} locale={locale} />

      <EmailNote title={t(locale, "signup-request.noteTitle")}>
        {t(locale, "signup-request.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default SignupRequest;
