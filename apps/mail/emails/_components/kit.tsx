import { Link, Section, Text } from "@react-email/components";
import type { ReactNode } from "react";

import { type Locale, t } from "./i18n";
import {
  buttonCellStyle,
  buttonSectionStyle,
  buttonStyle,
  colors,
  fallbackLabelStyle,
  fallbackLinkStyle,
  fallbackStyle,
  noteStyle,
  noteTextStyle,
  noteTitleStyle,
} from "./theme";

interface EmailButtonProps {
  href: string;
  children: ReactNode;
}

/**
 * Primary call to action. The background and padding live on a table cell so
 * the button keeps its shape in clients that ignore padding on links.
 */
export function EmailButton({ href, children }: EmailButtonProps) {
  return (
    <Section style={buttonSectionStyle}>
      <table
        role="presentation"
        border={0}
        cellPadding={0}
        cellSpacing={0}
        style={{ margin: "0 auto", borderCollapse: "separate" }}
      >
        <tbody>
          <tr>
            <td style={buttonCellStyle}>
              <a href={href} style={buttonStyle} target="_blank">
                {children}
              </a>
            </td>
          </tr>
        </tbody>
      </table>
    </Section>
  );
}

/**
 * Plain-text copy of the action URL, for clients that block buttons or for
 * readers who prefer copying the link.
 */
export function EmailFallbackLink({
  href,
  locale,
}: {
  href: string;
  locale: Locale;
}) {
  return (
    <Section style={fallbackStyle}>
      <Text style={fallbackLabelStyle}>
        {t(locale, "common.fallbackLabel")}
      </Text>
      <Link href={href} style={fallbackLinkStyle}>
        {href}
      </Link>
    </Section>
  );
}

interface EmailNoteProps {
  title: string;
  tone?: "neutral" | "warning";
  children: ReactNode;
}

/** Contextual note, used for security guidance and next steps. */
export function EmailNote({
  title,
  tone = "neutral",
  children,
}: EmailNoteProps) {
  const isWarning = tone === "warning";
  const accent = isWarning ? colors.warning : colors.accent;

  return (
    <Section
      style={{
        ...noteStyle,
        backgroundColor: isWarning
          ? colors.warningSurface
          : colors.accentSurface,
        borderLeft: `3px solid ${accent}`,
      }}
    >
      <Text
        style={{
          ...noteTitleStyle,
          color: isWarning ? colors.warningInk : colors.inkSoft,
        }}
      >
        {title}
      </Text>
      <Text style={noteTextStyle}>{children}</Text>
    </Section>
  );
}
