import { useCallback, useEffect, useState } from "react";
import {
  ActionIcon,
  Button,
  Group,
  Loader,
  Select,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import { AiOutlineDelete } from "react-icons/ai";
import { useTranslations } from "@/i18n";
import services from "@/services";
import type { Organization, UserOrganization } from "@/services/types";
import { AsyncSearchSelect, showNotification } from "@/components";
import type { AsyncSearchSelectOption } from "@/components/async-search-select/async-search-select";

const ROLE_OPTIONS = ["owner", "admin", "member"];
const DEFAULT_ROLE = "member";

interface Props {
  userId: string;
}

const UserOrganizations: React.FC<Props> = ({ userId }) => {
  const t = useTranslations();
  const [memberships, setMemberships] = useState<UserOrganization[] | null>(
    null,
  );
  const [newOrgId, setNewOrgId] = useState<string | null>(null);
  const [newRole, setNewRole] = useState<string>(DEFAULT_ROLE);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    const res = await services.admin.getUserOrganizations(userId);
    if (res.error) {
      showNotification({ type: "error", message: res.message });
      return;
    }
    setMemberships(res.data ?? []);
  }, [userId]);

  useEffect(() => {
    setMemberships(null);
    refresh();
  }, [refresh]);

  const upsertMembership = async (orgId: string, role: string) => {
    setSaving(true);
    const res = await services.admin.addUserToOrganization(userId, orgId, {
      role,
    });
    setSaving(false);
    if (res.error) {
      showNotification({ type: "error", message: res.message });
      return false;
    }
    await refresh();
    return true;
  };

  const handleAdd = async () => {
    if (!newOrgId) return;
    if (await upsertMembership(newOrgId, newRole)) {
      setNewOrgId(null);
      setNewRole(DEFAULT_ROLE);
    }
  };

  const handleRemove = async (orgId: string) => {
    setSaving(true);
    const res = await services.admin.removeUserFromOrganization(userId, orgId);
    setSaving(false);
    if (res.error) {
      showNotification({ type: "error", message: res.message });
      return;
    }
    // Removal also revokes the user's org-bound API keys and kills their
    // sessions acting in this org.
    showNotification({ type: "success", message: res.data?.message });
    await refresh();
  };

  const fetchOrganizations = async (
    query: string,
  ): Promise<AsyncSearchSelectOption[]> => {
    const res = await services.admin.getOrganizations({
      q: query || undefined,
      limit: 20,
    });
    const memberOf = new Set((memberships ?? []).map((m) => m.id));
    return (res.data?.data ?? [])
      .filter((org: Organization) => !memberOf.has(org.id))
      .map((org: Organization) => ({
        value: org.id,
        label: org.name,
      }));
  };

  const roleSelectData = (role: string) =>
    ROLE_OPTIONS.includes(role) ? ROLE_OPTIONS : [role, ...ROLE_OPTIONS];

  return (
    <Stack p="sm" gap="sm">
      <Text size="sm" fw={700}>
        {t("organizations")}
      </Text>

      {memberships === null ? (
        <Group justify="center" p="sm">
          <Loader size="sm" />
        </Group>
      ) : memberships.length === 0 ? (
        <Text size="sm" c="dimmed">
          {t("noOrganizations")}
        </Text>
      ) : (
        <Stack gap="xs">
          {memberships.map((membership) => (
            <Group key={membership.id} justify="space-between" wrap="nowrap">
              <Stack gap={0} style={{ flex: 1, minWidth: 0 }}>
                <Text size="sm" truncate>
                  {membership.name}
                </Text>
                {membership.memberSince && (
                  <Text size="xs" c="dimmed">
                    {t("memberSince")}{" "}
                    {new Date(membership.memberSince).toLocaleDateString()}
                  </Text>
                )}
              </Stack>
              <Select
                size="xs"
                w={110}
                variant="filled"
                disabled={saving}
                data={roleSelectData(membership.role)}
                value={membership.role}
                allowDeselect={false}
                onChange={(role) => {
                  if (role && role !== membership.role) {
                    upsertMembership(membership.id, role);
                  }
                }}
              />
              <Tooltip label={t("removeFromOrganization")} position="left">
                <ActionIcon
                  color="red"
                  variant="light"
                  disabled={saving}
                  onClick={() => handleRemove(membership.id)}
                >
                  <AiOutlineDelete />
                </ActionIcon>
              </Tooltip>
            </Group>
          ))}
        </Stack>
      )}

      <Group align="flex-end" wrap="nowrap" gap="xs">
        <div style={{ flex: 1 }}>
          <AsyncSearchSelect
            label={t("addToOrganization")}
            placeholder={t("selectOrganization")}
            fetchFn={fetchOrganizations}
            value={newOrgId}
            onChange={setNewOrgId}
          />
        </div>
        <Select
          size="sm"
          w={110}
          variant="filled"
          label={t("role")}
          data={ROLE_OPTIONS}
          value={newRole}
          allowDeselect={false}
          onChange={(role) => setNewRole(role ?? DEFAULT_ROLE)}
        />
        <Button
          size="sm"
          variant="light"
          disabled={!newOrgId || saving}
          onClick={handleAdd}
        >
          {t("add")}
        </Button>
      </Group>
    </Stack>
  );
};

export default UserOrganizations;
