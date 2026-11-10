import { Section, Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailNote } from "./_components/kit";
import {
  codeExpiryStyle,
  codePanelStyle,
  codeStyle,
  leadStyle,
} from "./_components/theme";

interface EmailOtpProps {
  code?: string;
  minutes?: string;
  locale?: Locale;
}

export const EmailOtp = ({
  code = "{{code}}",
  minutes = "{{minutes}}",
  locale = "en",
}: EmailOtpProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "email-otp.preview")}
      eyebrow={t(locale, "email-otp.eyebrow")}
      title={t(locale, "email-otp.title")}
    >
      <Text style={leadStyle}>{t(locale, "email-otp.lead")}</Text>

      <Section style={codePanelStyle}>
        <Text style={codeStyle}>{code}</Text>
      </Section>

      <Text style={codeExpiryStyle}>
        {t(locale, "email-otp.expiry").replace("{{minutes}}", minutes)}
      </Text>

      <EmailNote title={t(locale, "email-otp.noteTitle")}>
        {t(locale, "email-otp.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default EmailOtp;
