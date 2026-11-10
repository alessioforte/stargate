import {
  Body,
  Container,
  Head,
  Heading,
  Html,
  Preview,
  Section,
  Text,
} from "@react-email/components";
import type { ReactNode } from "react";

import { type Locale, t } from "./i18n";
import {
  bodyStyle,
  brandNameStyle,
  brandTaglineStyle,
  containerStyle,
  contentStyle,
  eyebrowDotStyle,
  eyebrowStyle,
  footerBrandStyle,
  footerLegalStyle,
  footerMetaStyle,
  footerStyle,
  footerTaglineStyle,
  headerStyle,
  headingStyle,
  logoMarkStyle,
} from "./theme";

interface EmailLayoutProps {
  locale: Locale;
  /** Text shown in the inbox preview, next to the subject. */
  preview: string;
  /** Small label above the title, e.g. "Account security". */
  eyebrow: string;
  title: string;
  children: ReactNode;
}

/**
 * Shared shell for every transactional email: dark branded header, white
 * content card, warm gold accents, and a neutral footer. Templates only
 * provide their preview text, eyebrow, title, and body content.
 */
export function EmailLayout({
  locale,
  preview,
  eyebrow,
  title,
  children,
}: EmailLayoutProps) {
  return (
    <Html lang={locale}>
      <Head>
        <meta name="color-scheme" content="light" />
        <meta name="supported-color-schemes" content="light" />
      </Head>
      <Body style={bodyStyle}>
        <Preview>{preview}</Preview>
        <Container style={containerStyle}>
          <Section style={headerStyle}>
            <span style={logoMarkStyle}>S</span>
            <span style={brandNameStyle}>Stargate</span>
            <span style={brandTaglineStyle}>
              {t(locale, "common.brandTagline")}
            </span>
          </Section>
          <Section style={contentStyle}>
            <Text style={eyebrowStyle}>
              <span style={eyebrowDotStyle} />
              {eyebrow}
            </Text>
            <Heading style={headingStyle}>{title}</Heading>
            {children}
          </Section>
          <Section style={footerStyle}>
            <Text style={footerBrandStyle}>Stargate</Text>
            <Text style={footerTaglineStyle}>
              {t(locale, "common.footerTagline")}
            </Text>
            <Text style={footerLegalStyle}>
              {t(locale, "common.footerLegal")}
            </Text>
            <Text style={footerMetaStyle}>
              {t(locale, "common.footerMeta")}
            </Text>
          </Section>
        </Container>
      </Body>
    </Html>
  );
}
