import { useCallback, useEffect, useMemo, useState } from "react";
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
import useStore from "@/store";
import { AsyncSearchSelect } from "@/components";
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
  const {
    userOrganizations,
    getUserOrganizations,
    upsertUserOrganization,
    removeUserOrganization,
    searchOrganizationOptions,
  } = useStore();
  const [newOrgId, setNewOrgId] = useState<string | null>(null);
  const [newRole, setNewRole] = useState<string>(DEFAULT_ROLE);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingRole, setEditingRole] = useState<string>("");
  const [isAdding, setIsAdding] = useState(false);

  const memberships = useMemo(
    () =>
      userOrganizations.meta === userId ? (userOrganizations.data ?? []) : null,
    [userId, userOrganizations.data, userOrganizations.meta],
  );
  const saving = userOrganizations.isLoading();

  useEffect(() => {
    void getUserOrganizations(userId);
  }, [getUserOrganizations, userId]);

  const upsertMembership = async (orgId: string, role: string) => {
    return upsertUserOrganization(userId, orgId, role);
  };

  const handleAdd = async () => {
    if (!newOrgId) return false;
    if (await upsertMembership(newOrgId, newRole)) {
      setNewOrgId(null);
      setNewRole(DEFAULT_ROLE);
      return true;
    }
    return false;
  };

  const handleRemove = async (orgId: string) => {
    // Removal also revokes the user's org-bound API keys and kills their
    // sessions acting in this org.
    await removeUserOrganization(userId, orgId);
  };

  const fetchOrganizations = useCallback(
    (query: string): Promise<AsyncSearchSelectOption[]> =>
      searchOrganizationOptions(
        query,
        (memberships ?? []).map((membership) => membership.id),
      ),
    [memberships, searchOrganizationOptions],
  );

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
                      if (
                        !(await upsertMembership(membership.id, editingRole))
                      ) {
                        return;
                      }
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
            if (await handleAdd()) {
              setIsAdding(false);
            }
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
