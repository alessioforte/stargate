import { useMemo } from "react";
import {
  Avatar,
  Box,
  Menu,
  Group,
  ActionIcon,
  Center,
  Text,
  Switch,
  useMantineColorScheme,
  Stack,
} from "@mantine/core";
import { useColorScheme } from "@mantine/hooks";
import { CiLogout, CiLight, CiDark } from "react-icons/ci";
import { MdOutlineChevronLeft } from "react-icons/md";
import { useTranslations, getLanguages } from "@/i18n";
import type { AdminMe } from "@/services/types";
import { formatDate } from "@/lib/format-date";
import classes from "./user-menu.module.css";

function isSupportedLanguage(value: string): value is "en" | "it" {
  return value === "en" || value === "it";
}

interface Props {
  adminMe: AdminMe | null;
  language: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
}

const UserMenu: React.FC<Props> = ({
  adminMe,
  language,
  onLanguageChange,
  onLogout,
}) => {
  const t = useTranslations();
  const languages = useMemo(() => getLanguages(), []);

  return (
    <Menu position="bottom-end" offset={15}>
      <Menu.Target>
        <Center>
          <ActionIcon
            color="default"
            variant="transparent"
            className={classes.actionIcon}
          >
            {adminMe && (
              <Group align="center">
                <Stack gap={0} align="end">
                  <Text fw={600} size="sm">
                    {adminMe.user.name}
                  </Text>
                  <Text size="xs" c="dimmed">
                    {adminMe.user.email}
                  </Text>
                </Stack>
                <Avatar
                  color="dark"
                  src={adminMe.user.picture}
                  name={adminMe.user.name}
                />
              </Group>
            )}
          </ActionIcon>
        </Center>
      </Menu.Target>
      <Menu.Dropdown className={classes.dropdown}>
        {adminMe && (
          <>
            <Stack p="sm" gap="xs">
              <Group align="center" justify="space-between">
                <Text size="xs" c="dimmed">
                  {t("authenticatedAt")}
                </Text>
                <Text size="xs">
                  {adminMe.authentication.authenticatedAt
                    ? formatDate(
                        adminMe.authentication.authenticatedAt,
                        null,
                        language,
                      )
                    : "—"}
                </Text>
              </Group>
              <Group align="center" justify="space-between">
                <Text size="xs" c="dimmed">
                  {t("accessExpiresAt")}
                </Text>
                <Text size="xs">
                  {formatDate(
                    adminMe.authentication.tokenExpiresAt,
                    null,
                    language,
                  )}
                </Text>
              </Group>
            </Stack>
          </>
        )}
        <Menu.Divider />
        <Menu.Label>{t("settings")}</Menu.Label>
        <Menu.Item>
          <Group justify="space-between">
            <ColorSchemeSwitch />
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
                  onLanguageChange(value);
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
          onClick={onLogout}
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

const ColorSchemeSwitch = () => {
  const defaultColorScheme = useColorScheme();
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const theme = colorScheme === "auto" ? defaultColorScheme : colorScheme;
  return (
    <Switch
      size="sm"
      onChange={(event) => {
        const colorScheme = event.target.checked ? "dark" : "light";
        setColorScheme(colorScheme);
      }}
      styles={{ body: { display: "flex" } }}
      checked={theme === "dark"}
      onLabel={<CiDark size={12} />}
      offLabel={<CiLight size={12} />}
    />
  );
};
