import {
  ActionIcon,
  Button,
  Flex,
  Group,
  Select,
  Stack,
  Textarea,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type {
  CreateServiceAccountRequest,
  Organization,
} from "@/services/types";
import { EntityDrawer } from "@/components";
import { organizationOptions } from "@/lib/organization-options";

interface FormValues {
  name: string;
  description: string;
  orgId: string;
}

interface Props {
  organizations: Organization[];
  onSave: (serviceAccount: CreateServiceAccountRequest) => void;
}

const emptyFormValues: FormValues = {
  name: "",
  description: "",
  orgId: "",
};

const CreateServiceAccount: React.FC<Props> = ({ organizations, onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      name: (value) => (value.trim() ? undefined : t("nameRequired")),
    },
  });

  const handleSubmit = (values: FormValues) => {
    const payload: CreateServiceAccountRequest = {
      name: values.name.trim(),
      description: values.description.trim() || null,
      orgId: values.orgId.trim() || null,
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
      <Tooltip label={t("newServiceAccount")} position="left" offset={10}>
        <ActionIcon onClick={open}>
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer
        opened={opened}
        onClose={handleCancel}
        title={t("serviceAccount")}
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
              <Select
                searchable
                clearable
                variant="filled"
                label={t("organization")}
                placeholder={t("selectOrganization")}
                data={organizationOptions(organizations)}
                value={form.values.orgId || null}
                onChange={(value) => form.setFieldValue("orgId", value ?? "")}
              />
            </Stack>
          </Flex>
        </form>
      </EntityDrawer>
    </>
  );
};

export default CreateServiceAccount;
