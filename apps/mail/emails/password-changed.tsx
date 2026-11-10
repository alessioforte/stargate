import { Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailNote } from "./_components/kit";
import { leadStyle, paragraphLastStyle } from "./_components/theme";

interface PasswordChangedProps {
  locale?: Locale;
}

export const PasswordChanged = ({ locale = "en" }: PasswordChangedProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "password-changed.preview")}
      eyebrow={t(locale, "password-changed.eyebrow")}
      title={t(locale, "password-changed.title")}
    >
      <Text style={leadStyle}>{t(locale, "password-changed.lead")}</Text>
      <Text style={paragraphLastStyle}>
        {t(locale, "password-changed.body")}
      </Text>

      <EmailNote title={t(locale, "password-changed.noteTitle")} tone="warning">
        {t(locale, "password-changed.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default PasswordChanged;
