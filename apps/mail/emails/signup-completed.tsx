import { Text } from "@react-email/components";

import { type Locale, t } from "./_components/i18n";
import { EmailLayout } from "./_components/layout";
import { EmailNote } from "./_components/kit";
import { leadStyle, paragraphLastStyle } from "./_components/theme";

interface SignupCompletedProps {
  locale?: Locale;
}

export const SignupCompleted = ({ locale = "en" }: SignupCompletedProps) => {
  return (
    <EmailLayout
      locale={locale}
      preview={t(locale, "signup-completed.preview")}
      eyebrow={t(locale, "signup-completed.eyebrow")}
      title={t(locale, "signup-completed.title")}
    >
      <Text style={leadStyle}>{t(locale, "signup-completed.lead")}</Text>
      <Text style={paragraphLastStyle}>
        {t(locale, "signup-completed.body")}
      </Text>

      <EmailNote title={t(locale, "signup-completed.noteTitle")}>
        {t(locale, "signup-completed.note")}
      </EmailNote>
    </EmailLayout>
  );
};

export default SignupCompleted;
