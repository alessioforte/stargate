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
import { useColorScheme } from "@mantine/hooks";
import { CiLogout, CiLight, CiDark } from "react-icons/ci";
import { MdOutlineChevronLeft } from "react-icons/md";
import { useTranslations, getLanguages } from "@/i18n";
import classes from "./user-menu.module.css";

function isSupportedLanguage(value: string): value is "en" | "it" {
  return value === "en" || value === "it";
}

interface Props {
  language: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
}

const UserMenu: React.FC<Props> = ({
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
