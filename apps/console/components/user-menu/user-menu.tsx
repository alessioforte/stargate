import { Avatar, Menu, Text, UnstyledButton } from "@mantine/core";
import { LuChevronDown, LuLogOut } from "react-icons/lu";
import { useTranslations, getLanguages } from "@/i18n";
import type { AdminMe } from "@/services/types";
import { formatDate } from "@/lib/format-date";
import classes from "./user-menu.module.css";

const languageNames: Record<string, string> = {
  en: "English",
  it: "Italiano",
};

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
  const name = adminMe?.user?.name ?? t("adminKey");
  const identifier = adminMe?.user?.email ?? adminMe?.adminKeyId;
  const authentication = adminMe?.authentication;

  return (
    <Menu position="bottom-end" offset={8} width={280}>
      <Menu.Target>
        <UnstyledButton className={classes.trigger} aria-label={name}>
          <Avatar
            size={28}
            color="brand"
            src={adminMe?.user?.picture}
            name={name}
          />
          <span className={classes.name}>{name}</span>
          <LuChevronDown size={14} className={classes.chevron} />
        </UnstyledButton>
      </Menu.Target>
      <Menu.Dropdown className={classes.dropdown}>
        {adminMe && (
          <>
            <div className={classes.identity}>
              <Avatar
                size={36}
                color="brand"
                src={adminMe.user?.picture}
                name={name}
              />
              <div className={classes.identityText}>
                <Text size="sm" fw={600} truncate>
                  {name}
                </Text>
                <Text size="xs" c="dimmed" truncate>
                  {identifier}
                </Text>
              </div>
            </div>
            <div className={classes.row}>
              <span className={classes.rowLabel}>{t("authenticatedAt")}</span>
              <span>
                {authentication?.authenticatedAt
                  ? formatDate(authentication.authenticatedAt, null, language)
                  : "—"}
              </span>
            </div>
            <div className={classes.row}>
              <span className={classes.rowLabel}>{t("accessExpiresAt")}</span>
              <span>
                {authentication?.tokenExpiresAt
                  ? formatDate(authentication.tokenExpiresAt, null, language)
                  : "—"}
              </span>
            </div>
            <Menu.Divider />
          </>
        )}
        <Menu.Label>{t("language")}</Menu.Label>
        <Menu.RadioGroup
          value={language}
          onChange={(value) => {
            if (isSupportedLanguage(value)) {
              onLanguageChange(value);
            }
          }}
        >
          {getLanguages().map((lang) => (
            <Menu.RadioItem key={lang} value={lang}>
              {languageNames[lang] ?? lang}
            </Menu.RadioItem>
          ))}
        </Menu.RadioGroup>
        <Menu.Divider />
        <Menu.Item
          color="red"
          leftSection={<LuLogOut size={15} />}
          onClick={onLogout}
        >
          {t("logout")}
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  );
};

export default UserMenu;
