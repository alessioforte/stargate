import { useState } from "react";
import { Box, Flex, Group, Text } from "@mantine/core";
import { useForm } from "@mantine/form";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import CodeBox from "../code-box/code-box";
import EditActionControls from "../edit-action-controls/edit-action-controls";

interface Props {
  attrs: string;
  onSave: (attrs: JsonValue) => void;
}

const JsonAttributesForm: React.FC<Props> = ({ attrs, onSave }) => {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<{ attrs: string }>({
    initialValues: { attrs },
    validate: {
      attrs: (value) => {
        try {
          JSON.parse(value);
          return null;
        } catch {
          return t("invalidJson");
        }
      },
    },
  });

  return (
    <form
      style={{ flex: 1 }}
      onSubmit={form.onSubmit((values) => onSave(JSON.parse(values.attrs)))}
    >
      <Flex h="100%" direction="column">
        <Group p="sm" justify="space-between">
          <Group gap="xs">
            <Text size="sm" fw={700}>
              {t("attributes")}
            </Text>
            {form.errors.attrs && <Text c="red">{form.errors.attrs}</Text>}
          </Group>
          <EditActionControls
            editing={edit}
            onCancel={() => {
              setEdit(false);
              form.reset();
            }}
            onEdit={() => setEdit(true)}
          />
        </Group>
        <Box style={{ flex: 1, height: "100%" }}>
          <CodeBox
            readOnly={!edit}
            value={form.values.attrs}
            onChange={(value) => form.setFieldValue("attrs", value)}
          />
        </Box>
      </Flex>
    </form>
  );
};

export default JsonAttributesForm;
