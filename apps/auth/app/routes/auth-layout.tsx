import { Box, Group } from "@mantine/core";
import { Outlet } from "react-router";
import { ColorSchemeToggle, LanguageSelect } from "@/components";
import useStore from "@/store";

export default function AuthLayout() {
  const theme = useStore((state) => state.theme);
  const language = useStore((state) => state.language);
  const setTheme = useStore((state) => state.setTheme);
  const setLanguage = useStore((state) => state.setLanguage);

  return (
    <Box className="auth-page">
      <Group className="auth-toolbar" gap="xs">
        <LanguageSelect
          value={language}
          options={[
            { label: "English", value: "en", href: "" },
            { label: "Italiano", value: "it", href: "" },
          ]}
          dropdownPosition="bottom-end"
          onChange={(value) => {
            if (value === "en" || value === "it") {
              setLanguage(value);
            }
          }}
        />
        <ColorSchemeToggle theme={theme} onClick={setTheme} />
      </Group>
      <main className="auth-main">
        <Box className="auth-panel">
          <Outlet />
        </Box>
      </main>
    </Box>
  );
}
