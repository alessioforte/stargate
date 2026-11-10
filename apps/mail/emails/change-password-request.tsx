import { Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailButton, EmailFallbackLink, EmailNote } from "./_components/kit";
import { leadStyle, paragraphLastStyle } from "./_components/theme";

interface ChangePasswordRequestProps {
  changePasswordUrl?: string;
  locale?: Locale;
}

export const ChangePasswordRequest = ({
  changePasswordUrl = "{{link}}",
  locale = "en",
}: ChangePasswordRequestProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "change-password-request.preview")}
      eyebrow={t(locale, "change-password-request.eyebrow")}
      title={t(locale, "change-password-request.title")}
    >
      <Text style={leadStyle}>{t(locale, "change-password-request.lead")}</Text>
      <Text style={paragraphLastStyle}>
        {t(locale, "change-password-request.body")}
      </Text>

      <EmailButton href={changePasswordUrl}>
        {t(locale, "change-password-request.button")}
      </EmailButton>
      <EmailFallbackLink href={changePasswordUrl} locale={locale} />

      <EmailNote title={t(locale, "change-password-request.noteTitle")}>
        {t(locale, "change-password-request.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default ChangePasswordRequest;
