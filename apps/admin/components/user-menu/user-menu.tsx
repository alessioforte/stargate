import { useMemo } from "react";
import {
  Box,
  Menu,
  Group,
  ActionIcon,
  Center,
  Text,
  Switch,
  useMantineColorScheme,
} from "@mantine/core";
import { CiLogout, CiLight, CiDark } from "react-icons/ci";
import { MdOutlineChevronLeft } from "react-icons/md";
import { useLocation } from "react-router";
import { useTranslations, getLanguages } from "@/i18n";
import useStore from "@/store";
import classes from "./user-menu.module.css";

function currentReturnPath(pathname: string, query: string) {
  return query ? `${pathname}${query}` : pathname;
}

function isSupportedLanguage(value: string): value is "en" | "it" {
  return value === "en" || value === "it";
}

const UserMenu = () => {
  const t = useTranslations();
  const location = useLocation();
  const logout = useStore((state) => state.logout);
  const theme = useStore((state) => state.theme);
  const setTheme = useStore((state) => state.setTheme);
  const language = useStore((state) => state.language);
  const setLanguage = useStore((state) => state.setLanguage);

  const { setColorScheme } = useMantineColorScheme();

  const languages = useMemo(() => getLanguages(), []);

  const handleLogout = () => {
    logout(currentReturnPath(location.pathname, location.search));
  };

  return (
    <Menu position="bottom-end" offset={15}>
      <Menu.Target>
        <Center>
          <ActionIcon
            color="default"
            variant="transparent"
            className={classes.actionIcon}
          >
            {/*<Box className={classes.labels}>
              <Text fw={600}>{profile?.full_name || profile?.email || ""}</Text>
              <Text className={classes.description} fw={500} tt="uppercase">
                {currentOrganization?.org_name || ""}
              </Text>
            </Box>*/}
            <Center className={classes.avatar}>
              <Text size="xl" className={classes.char}>
                A
              </Text>
            </Center>
          </ActionIcon>
        </Center>
      </Menu.Target>
      <Menu.Dropdown className={classes.dropdown}>
        <Menu.Label>{t("admin")}</Menu.Label>
        <Menu.Divider />
        <Menu.Item>
          <Group justify="space-between">
            <Switch
              size="sm"
              onChange={(event) => {
                const colorScheme = event.target.checked ? "dark" : "light";
                setTheme(colorScheme);
                setColorScheme(colorScheme);
              }}
              styles={{ body: { display: "flex" } }}
              checked={theme === "dark"}
              onLabel={<CiDark size={12} />}
              offLabel={<CiLight size={12} />}
            />
            {t("darkMode")}
          </Group>
        </Menu.Item>
        <Menu.Sub position="left-start" offset={10}>
          <Menu.Sub.Target>
            <Menu.Sub.Item
              pr={2}
              rightSection={<></>}
              leftSection={<MdOutlineChevronLeft size={16} />}
            >
              <Group justify="end">{t("language")}</Group>
            </Menu.Sub.Item>
          </Menu.Sub.Target>
          <Menu.Sub.Dropdown>
            <Menu.RadioGroup
              value={language}
              onChange={(value) => {
                if (isSupportedLanguage(value)) {
                  setLanguage(value);
                }
              }}
            >
              {languages.map((lang) => (
                <Menu.RadioItem key={lang} value={lang}>
                  <Group>
                    <Box
                      style={{
                        width: 20,
                        height: 20,
                        borderRadius: "50%",
                        backgroundImage: `url(https://flagcdn.com/${
                          lang === "en" ? "gb" : lang
                        }.svg)`,
                        backgroundRepeat: "no-repeat, repeat",
                        backgroundPosition: "center",
                        backgroundSize: "cover",
                      }}
                    />

                    <div>
                      <Text size="sm">{lang}</Text>
                    </div>
                  </Group>
                </Menu.RadioItem>
              ))}
            </Menu.RadioGroup>
          </Menu.Sub.Dropdown>
        </Menu.Sub>
        <Menu.Divider />
        <Menu.Item
          onClick={handleLogout}
          leftSection={<CiLogout size={16} />}
          color="red"
        >
          <Group justify="end">{t("logout")}</Group>
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  );
};

export default UserMenu;
