import { Box, Group } from "@mantine/core";
import { useEffect } from "react";
import { Outlet } from "react-router";
import { ColorSchemeToggle, LanguageSelect } from "@/components";
import { TranslationProvider } from "@/i18n";
import useStore from "@/store";

export default function AuthLayout() {
  const theme = useStore((state) => state.theme);
  const language = useStore((state) => state.language);
  const setTheme = useStore((state) => state.setTheme);
  const setLanguage = useStore((state) => state.setLanguage);
  const loadApiMessages = useStore((state) => state.loadApiMessages);

  useEffect(() => {
    void loadApiMessages(language === "it" ? "it" : "en");
  }, [language, loadApiMessages]);

  return (
    <TranslationProvider language={language}>
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
    </TranslationProvider>
  );
}
