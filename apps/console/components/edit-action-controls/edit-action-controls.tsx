import type { ComponentProps } from "react";
import { ActionIcon, Button, Group } from "@mantine/core";
import { FiEdit2 } from "react-icons/fi";
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
            color="gray"
          >
            {t("cancel")}
          </Button>
          <Button type="submit" size="compact-sm" color="teal">
            {t("save")}
          </Button>
        </>
      )}
      {!editing && (
        <ActionIcon type="button" color="blue" variant="light" onClick={onEdit}>
          <FiEdit2 />
        </ActionIcon>
      )}
    </Group>
  );
};

export default EditActionControls;
