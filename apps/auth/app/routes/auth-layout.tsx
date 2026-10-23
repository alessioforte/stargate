import { Box, Group } from "@mantine/core";
import { useEffect } from "react";
import { Outlet } from "react-router";
import { ColorSchemeToggle, LanguageSelect } from "@/components";
import { TranslationProvider } from "@/i18n";
import useStore from "@/store";

export default function AuthLayout() {
  const language = useStore((state) => state.language);
  const setLanguage = useStore((state) => state.setLanguage);
  const loadApiMessages = useStore((state) => state.loadApiMessages);
  const loadFeatures = useStore((state) => state.loadFeatures);

  useEffect(() => {
    void loadApiMessages(language === "it" ? "it" : "en");
  }, [language, loadApiMessages]);

  // Effects run after hydration, so prerendered pages render without the
  // features first, as they were prerendered.
  useEffect(() => {
    void loadFeatures();
  }, [loadFeatures]);

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
          <ColorSchemeToggle />
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
