import {
  Box,
  ActionIcon,
  Button,
  Divider,
  TextInput,
  Textarea,
  Stack,
  Flex,
  Group,
  Text,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type { CreateOrganizationRequest } from "@/services/types";
import { CodeBox, EntityDrawer } from "@/components";

interface FormValues {
  name: string;
  description: string;
  attrs: string;
}

interface Props {
  onSave: (organization: CreateOrganizationRequest) => void;
}

const emptyFormValues: FormValues = {
  name: "",
  description: "",
  attrs: "{}",
};

const CreateOrganization: React.FC<Props> = ({ onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      name: (value) => (value ? undefined : "Name is required"),
      attrs: (value) => {
        try {
          JSON.parse(value);
          return undefined;
        } catch {
          return t("invalidJson");
        }
      },
    },
  });

  const handleSubmit = (values: FormValues) => {
    const payload: CreateOrganizationRequest = {
      name: values.name.trim(),
      description: values.description.trim() || null,
      attrs: JSON.parse(values.attrs) ?? {},
    };

    onSave(payload);
    form.reset();
    close();
  };

  const handleCancel = () => {
    form.reset();
    close();
  };

  return (
    <>
      <Tooltip label={t("newOrganization")} position="left" offset={10}>
        <ActionIcon
          onClick={() => {
            open();
          }}
        >
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer
        opened={opened}
        onClose={handleCancel}
        title={t("organization")}
      >
        <form style={{ flex: 1 }} onSubmit={form.onSubmit(handleSubmit)}>
          <Flex
            direction="column"
            justify="space-between"
            style={{ height: "100%" }}
          >
            <Group mt="sm" px="sm" justify="flex-end" h={28}>
              <Button type="submit" size="compact-sm" color="teal">
                {t("save")}
              </Button>
            </Group>

            <Flex direction="column" justify="space-between">
              <Stack p="sm">
                <TextInput
                  variant="filled"
                  label={t("name")}
                  {...form.getInputProps("name")}
                />
                <Textarea
                  autosize
                  minRows={3}
                  variant="filled"
                  label={t("description")}
                  {...form.getInputProps("description")}
                />
              </Stack>
            </Flex>

            <Divider my="md" />

            <Group gap="xs">
              <Text size="sm" fw={700}>
                {t("attributes")}
              </Text>
              {form.errors.attrs && <Text c="red">{form.errors.attrs}</Text>}
            </Group>

            <CodeBox
              value={form.values.attrs}
              onChange={(value) => form.setFieldValue("attrs", value)}
            />
          </Flex>
        </form>
      </EntityDrawer>
    </>
  );
};

export default CreateOrganization;
