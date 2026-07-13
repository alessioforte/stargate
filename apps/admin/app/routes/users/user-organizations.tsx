import { useCallback, useEffect, useState } from "react";
import {
  Badge,
  ActionIcon,
  Group,
  Loader,
  Select,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import {
  AiOutlineCheck,
  AiOutlineClose,
  AiOutlineDelete,
  AiOutlinePlus,
} from "react-icons/ai";
import { FiEdit2 } from "react-icons/fi";
import { useTranslations } from "@/i18n";
import services from "@/services";
import type { Organization, UserOrganization } from "@/services/types";
import { AsyncSearchSelect, showNotification } from "@/components";
import type { AsyncSearchSelectOption } from "@/components/async-search-select/async-search-select";

const ROLE_OPTIONS = ["owner", "admin", "member"];
const DEFAULT_ROLE = "member";

interface InlineEditorProps {
  disabled: boolean;
  onSave: () => void;
  onCancel: () => void;
  children: React.ReactNode;
}

function InlineEditor({
  disabled,
  onSave,
  onCancel,
  children,
}: InlineEditorProps) {
  return (
    <Group gap="xs" wrap="nowrap" align="flex-end">
      {children}
      <Group gap={5}>
        <ActionIcon color="gray" onClick={onCancel}>
          <AiOutlineClose />
        </ActionIcon>
        <ActionIcon color="green" disabled={disabled} onClick={onSave}>
          <AiOutlineCheck />
        </ActionIcon>
      </Group>
    </Group>
  );
}

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
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingRole, setEditingRole] = useState<string>("");
  const [isAdding, setIsAdding] = useState(false);

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

  return (
    <Stack p="sm" gap="sm">
      <Group justify="space-between" align="center">
        <Text size="sm" fw={700}>
          {t("organizations")}
        </Text>
        <ActionIcon onClick={() => setIsAdding((v) => !v)}>
          <AiOutlinePlus />
        </ActionIcon>
      </Group>

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
            <Group
              key={membership.id}
              justify="space-between"
              align="center"
              wrap="nowrap"
            >
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
              {editingId === membership.id && (
                <InlineEditor
                  disabled={saving}
                  onSave={async () => {
                    if (editingRole !== membership.role) {
                      await upsertMembership(membership.id, editingRole);
                    }
                    setEditingId(null);
                  }}
                  onCancel={() => setEditingId(null)}
                >
                  <Select
                    size="xs"
                    w={110}
                    variant="filled"
                    disabled={saving}
                    data={
                      ROLE_OPTIONS.includes(membership.role)
                        ? ROLE_OPTIONS
                        : [membership.role, ...ROLE_OPTIONS]
                    }
                    value={editingRole}
                    allowDeselect={false}
                    onChange={(role) => role && setEditingRole(role)}
                  />
                </InlineEditor>
              )}
              {editingId !== membership.id && (
                <>
                  <Badge color="teal" size="lg">
                    {membership.role}
                  </Badge>
                  <Group gap={5}>
                    <ActionIcon
                      variant="light"
                      color="blue"
                      disabled={saving}
                      onClick={() => {
                        setEditingId(membership.id);
                        setEditingRole(membership.role);
                      }}
                    >
                      <FiEdit2 />
                    </ActionIcon>
                    <Tooltip
                      label={t("removeFromOrganization")}
                      position="left"
                    >
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
                </>
              )}
            </Group>
          ))}
        </Stack>
      )}

      {isAdding && (
        <InlineEditor
          disabled={saving || !newOrgId}
          onSave={async () => {
            await handleAdd();
            setIsAdding(false);
          }}
          onCancel={() => {
            setNewOrgId(null);
            setNewRole(DEFAULT_ROLE);
            setIsAdding(false);
          }}
        >
          <div style={{ flex: 1 }}>
            <AsyncSearchSelect
              size="xs"
              label={t("organization")}
              placeholder={t("selectOrganization")}
              fetchFn={fetchOrganizations}
              value={newOrgId}
              onChange={setNewOrgId}
            />
          </div>
          <Select
            size="xs"
            w={110}
            variant="filled"
            label={t("role")}
            data={ROLE_OPTIONS}
            value={newRole}
            allowDeselect={false}
            onChange={(role) => setNewRole(role ?? DEFAULT_ROLE)}
          />
        </InlineEditor>
      )}
    </Stack>
  );
};

export default UserOrganizations;
