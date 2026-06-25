import { Menu, ActionIcon, Center, Text } from "@mantine/core";
import { CiLogout } from "react-icons/ci";
import { useLocation } from "react-router";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import classes from "./user-menu.module.css";

function currentReturnPath(pathname: string, query: string) {
  return query ? `${pathname}${query}` : pathname;
}

const UserMenu = () => {
  const t = useTranslations();
  const location = useLocation();
  const logout = useStore((state) => state.logout);

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
        <Menu.Label>{t("account")}</Menu.Label>
        <Menu.Divider />
        <Menu.Item
          onClick={handleLogout}
          leftSection={<CiLogout size={16} />}
          color="red"
        >
          {t("logout")}
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  );
};

export default UserMenu;
