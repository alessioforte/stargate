import {
  ActionIcon,
  Alert,
  Badge,
  Button,
  Card,
  Code,
  createTheme,
  Drawer,
  Menu,
  Modal,
  Skeleton,
  Tabs,
  Tooltip,
  type MantineColorsTuple,
} from "@mantine/core";

const brand: MantineColorsTuple = [
  "#eef4ff",
  "#d9e6fc",
  "#b0caf4",
  "#84abec",
  "#5f91e5",
  "#4480e1",
  "#2a6bd9",
  "#0f55c4",
  "#0a47a8",
  "#063a8c",
];

// Neutral, slightly cool surfaces for the dark scheme.
const dark: MantineColorsTuple = [
  "#d5d9df",
  "#a8aeb8",
  "#868d98",
  "#5a616c",
  "#363b44",
  "#2a2e35",
  "#1e2126",
  "#17191d",
  "#111316",
  "#0b0c0e",
];

const sans =
  '"Inter Variable", Inter, ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif';
const mono =
  '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace';

export const theme = createTheme({
  primaryColor: "brand",
  primaryShade: { light: 7, dark: 6 },
  colors: { brand, dark },
  fontFamily: sans,
  fontFamilyMonospace: mono,
  defaultRadius: "md",
  radius: { xs: "3px", sm: "4px", md: "6px", lg: "10px", xl: "16px" },
  cursorType: "pointer",
  headings: {
    fontFamily: sans,
    fontWeight: "600",
    sizes: {
      h1: { fontSize: "26px", lineHeight: "1.25" },
      h2: { fontSize: "22px", lineHeight: "1.3" },
      h3: { fontSize: "18px", lineHeight: "1.35" },
      h4: { fontSize: "16px", lineHeight: "1.4" },
      h5: { fontSize: "14px", lineHeight: "1.45" },
      h6: { fontSize: "13px", lineHeight: "1.45" },
    },
  },
  components: {
    ActionIcon: ActionIcon.extend({
      defaultProps: { variant: "subtle" },
    }),
    Alert: Alert.extend({
      defaultProps: { variant: "light" },
    }),
    Badge: Badge.extend({
      defaultProps: { radius: "sm", variant: "light" },
      styles: {
        label: { textTransform: "none", fontWeight: 500, letterSpacing: 0 },
      },
    }),
    Button: Button.extend({
      styles: { label: { fontWeight: 500 } },
    }),
    Card: Card.extend({
      defaultProps: { radius: "lg", withBorder: true },
    }),
    Code: Code.extend({
      styles: { root: { fontSize: "12px" } },
    }),
    Drawer: Drawer.extend({
      defaultProps: {
        overlayProps: { backgroundOpacity: 0.35, blur: 2 },
      },
    }),
    Menu: Menu.extend({
      defaultProps: { shadow: "md", radius: "md" },
    }),
    Modal: Modal.extend({
      defaultProps: {
        centered: true,
        radius: "lg",
        overlayProps: { backgroundOpacity: 0.35, blur: 2 },
      },
    }),
    Skeleton: Skeleton.extend({
      defaultProps: { radius: "sm" },
    }),
    Tabs: Tabs.extend({
      styles: { tab: { fontWeight: 500 } },
    }),
    Tooltip: Tooltip.extend({
      defaultProps: { withArrow: true, openDelay: 250 },
    }),
  },
});
