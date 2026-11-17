import type { ComponentProps } from "react";
import { ActionIcon, Button, Group } from "@mantine/core";
import { LuPencil } from "react-icons/lu";
import { useTranslations } from "@/i18n";

type Props = Omit<ComponentProps<typeof Group>, "children"> & {
  editing: boolean;
  onCancel: () => void;
  onEdit: () => void;
};

const EditActionControls: React.FC<Props> = ({
  editing,
  onCancel,
  onEdit,
  gap = "xs",
  h = 28,
  ...groupProps
}) => {
  const t = useTranslations();

  return (
    <Group gap={gap} h={h} {...groupProps}>
      {editing && (
        <>
          <Button
            type="button"
            size="compact-sm"
            onClick={onCancel}
            variant="default"
          >
            {t("cancel")}
          </Button>
          <Button type="submit" size="compact-sm">
            {t("save")}
          </Button>
        </>
      )}
      {!editing && (
        <ActionIcon
          type="button"
          variant="light"
          aria-label={t("edit")}
          onClick={onEdit}
        >
          <LuPencil size={14} />
        </ActionIcon>
      )}
    </Group>
  );
};

export default EditActionControls;
