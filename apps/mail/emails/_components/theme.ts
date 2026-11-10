import type { CSSProperties } from "react";

/**
 * Shared design tokens for every transactional email. Keep the palette aligned
 * with Stargate's console: ink for structure, warm gold for accents, neutral
 * greys for text and surfaces.
 */
export const colors = {
  ink: "#101828",
  inkSoft: "#344054",
  body: "#475467",
  muted: "#667085",
  faint: "#98A2B3",
  border: "#EAECF0",
  surface: "#F9FAFB",
  page: "#F2F4F7",
  white: "#FFFFFF",
  accent: "#FAB005",
  accentStrong: "#B54708",
  accentSurface: "#FFFAEB",
  warning: "#D92D20",
  warningInk: "#B42318",
  warningSurface: "#FEF3F2",
} as const;

export const fontFamily =
  "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif";

export const monoFontFamily =
  "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace";

export const bodyStyle: CSSProperties = {
  backgroundColor: colors.page,
  color: colors.body,
  fontFamily,
  margin: 0,
  padding: "32px 12px",
  WebkitFontSmoothing: "antialiased",
};

export const containerStyle: CSSProperties = {
  width: "100%",
  maxWidth: "600px",
  margin: "0 auto",
  backgroundColor: colors.white,
  border: `1px solid ${colors.border}`,
  borderRadius: "16px",
  boxShadow: "0 1px 3px rgba(16, 24, 40, 0.08)",
  overflow: "hidden",
};

export const headerStyle: CSSProperties = {
  backgroundColor: colors.ink,
  padding: "24px 32px",
  borderBottom: `3px solid ${colors.accent}`,
  borderTopLeftRadius: "15px",
  borderTopRightRadius: "15px",
};

export const logoMarkStyle: CSSProperties = {
  display: "inline-block",
  width: "32px",
  height: "32px",
  lineHeight: "32px",
  borderRadius: "9px",
  backgroundColor: colors.accent,
  color: colors.ink,
  fontFamily,
  fontSize: "17px",
  fontWeight: 700,
  textAlign: "center",
  verticalAlign: "middle",
};

export const brandNameStyle: CSSProperties = {
  display: "inline-block",
  marginLeft: "12px",
  color: colors.white,
  fontFamily,
  fontSize: "18px",
  fontWeight: 600,
  letterSpacing: "-0.02em",
  verticalAlign: "middle",
};

export const brandTaglineStyle: CSSProperties = {
  display: "block",
  marginTop: "2px",
  marginLeft: "44px",
  color: colors.faint,
  fontFamily,
  fontSize: "10px",
  fontWeight: 500,
  letterSpacing: "0.16em",
  textTransform: "uppercase",
};

export const contentStyle: CSSProperties = {
  padding: "36px 32px 28px",
};

export const eyebrowStyle: CSSProperties = {
  margin: "0 0 10px",
  color: colors.accentStrong,
  fontFamily,
  fontSize: "11px",
  fontWeight: 600,
  letterSpacing: "0.14em",
  lineHeight: "16px",
  textTransform: "uppercase",
};

export const eyebrowDotStyle: CSSProperties = {
  display: "inline-block",
  width: "6px",
  height: "6px",
  marginRight: "8px",
  borderRadius: "50%",
  backgroundColor: colors.accent,
  verticalAlign: "middle",
};

export const headingStyle: CSSProperties = {
  margin: "0 0 16px",
  color: colors.ink,
  fontFamily,
  fontSize: "26px",
  fontWeight: 600,
  letterSpacing: "-0.02em",
  lineHeight: "34px",
};

export const leadStyle: CSSProperties = {
  margin: "0 0 14px",
  color: colors.inkSoft,
  fontFamily,
  fontSize: "16px",
  lineHeight: "26px",
};

export const paragraphStyle: CSSProperties = {
  margin: "0 0 14px",
  color: colors.body,
  fontFamily,
  fontSize: "15px",
  lineHeight: "24px",
};

export const paragraphLastStyle: CSSProperties = {
  ...paragraphStyle,
  marginBottom: 0,
};

export const codePanelStyle: CSSProperties = {
  margin: "26px 0 0",
  padding: "22px 16px",
  backgroundColor: colors.ink,
  borderRadius: "12px",
  textAlign: "center",
};

export const codeStyle: CSSProperties = {
  margin: 0,
  color: colors.accent,
  fontFamily: monoFontFamily,
  fontSize: "30px",
  fontWeight: 700,
  letterSpacing: "0.28em",
  lineHeight: "40px",
  textAlign: "center",
  wordBreak: "break-all",
};

export const codeExpiryStyle: CSSProperties = {
  margin: "14px 0 0",
  color: colors.muted,
  fontFamily,
  fontSize: "13px",
  lineHeight: "20px",
  textAlign: "center",
};

export const buttonSectionStyle: CSSProperties = {
  margin: "26px 0 0",
  textAlign: "center",
};

export const buttonCellStyle: CSSProperties = {
  backgroundColor: colors.accent,
  borderRadius: "10px",
  padding: "14px 28px",
  textAlign: "center",
};

export const buttonStyle: CSSProperties = {
  color: colors.ink,
  display: "inline-block",
  fontFamily,
  fontSize: "15px",
  fontWeight: 600,
  letterSpacing: "0.01em",
  lineHeight: "20px",
  textDecoration: "none",
};

export const fallbackStyle: CSSProperties = {
  margin: "24px 0 0",
  padding: "14px 16px",
  backgroundColor: colors.surface,
  border: `1px dashed #D0D5DD`,
  borderRadius: "10px",
};

export const fallbackLabelStyle: CSSProperties = {
  margin: "0 0 4px",
  color: colors.muted,
  fontFamily,
  fontSize: "11px",
  fontWeight: 600,
  letterSpacing: "0.08em",
  lineHeight: "16px",
  textTransform: "uppercase",
};

export const fallbackLinkStyle: CSSProperties = {
  color: colors.inkSoft,
  fontFamily,
  fontSize: "12px",
  lineHeight: "18px",
  textDecoration: "underline",
  wordBreak: "break-all",
};

export const noteStyle: CSSProperties = {
  margin: "24px 0 0",
  padding: "16px 18px",
  backgroundColor: colors.surface,
  borderRadius: "10px",
};

export const noteTitleStyle: CSSProperties = {
  margin: "0 0 4px",
  color: colors.inkSoft,
  fontFamily,
  fontSize: "11px",
  fontWeight: 600,
  letterSpacing: "0.08em",
  lineHeight: "16px",
  textTransform: "uppercase",
};

export const noteTextStyle: CSSProperties = {
  margin: 0,
  color: colors.body,
  fontFamily,
  fontSize: "13px",
  lineHeight: "20px",
};

export const footerStyle: CSSProperties = {
  padding: "22px 32px",
  backgroundColor: colors.surface,
  borderTop: `1px solid ${colors.border}`,
  borderBottomLeftRadius: "15px",
  borderBottomRightRadius: "15px",
  textAlign: "center",
};

export const footerBrandStyle: CSSProperties = {
  margin: "0 0 4px",
  color: colors.ink,
  fontFamily,
  fontSize: "13px",
  fontWeight: 600,
  letterSpacing: "-0.01em",
};

export const footerTaglineStyle: CSSProperties = {
  margin: "0 0 10px",
  color: colors.muted,
  fontFamily,
  fontSize: "12px",
  lineHeight: "18px",
};

export const footerLegalStyle: CSSProperties = {
  margin: "0 0 2px",
  color: colors.faint,
  fontFamily,
  fontSize: "11px",
  lineHeight: "16px",
};

export const footerMetaStyle: CSSProperties = {
  margin: 0,
  color: colors.faint,
  fontFamily,
  fontSize: "11px",
  lineHeight: "16px",
};
