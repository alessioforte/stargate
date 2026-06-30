import { useState } from "react";
import { Divider, TextInput, Textarea, Stack, Flex } from "@mantine/core";
import { useForm } from "@mantine/form";
import { useTranslations } from "@/i18n";
import type { UpdateOrganizationRequest, Organization } from "@/services/types";
import {
  EditActionControls,
  EntityDrawer,
  JsonAttributesForm,
} from "@/components";

interface FormValues {
  id: string;
  name: string;
  description: string;
}

interface Props {
  organization?: Organization | null;
  opened: boolean;
  onClose: () => void;
  onSave: (id: string, organization: UpdateOrganizationRequest) => void;
}

const emptyFormValues: FormValues = {
  id: "",
  name: "",
  description: "",
};

function organizationToFormValues(
  organization?: Organization | null,
): FormValues {
  if (!organization) return emptyFormValues;

  return {
    id: organization.id,
    name: organization.name,
    description: organization.description ?? "",
  };
}

function EditOrganizationForm({
  organization,
  onSave,
}: {
  organization?: Organization | null;
  onSave: (id: string, organization: UpdateOrganizationRequest) => void;
}) {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<FormValues>({
    initialValues: organizationToFormValues(organization),
    validate: {
      name: (value) => (value ? undefined : "Name is required"),
    },
  });

  const handleSubmit = (values: FormValues) => {
    if (!organization) return;

    const payload: UpdateOrganizationRequest = {
      name: values.name.trim(),
      description: values.description.trim() || null,
      attrs: organization?.attrs ?? null,
    };

    onSave(organization.id, payload);
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      <EditActionControls
        editing={edit}
        mt="sm"
        px="sm"
        justify="flex-end"
        onCancel={() => {
          setEdit(false);
          form.reset();
        }}
        onEdit={() => setEdit(true)}
      />

      <Flex direction="column" justify="space-between">
        <Stack p="sm">
          <TextInput
            readOnly={!edit}
            variant="filled"
            label={t("name")}
            {...form.getInputProps("name")}
          />
          <Textarea
            readOnly={!edit}
            autosize
            minRows={3}
            variant="filled"
            label={t("description")}
            {...form.getInputProps("description")}
          />
        </Stack>
      </Flex>
    </form>
  );
}

const EditOrganization: React.FC<Props> = ({
  organization,
  onClose,
  onSave,
  opened,
}) => {
  const t = useTranslations();
  return (
    <EntityDrawer opened={opened} onClose={onClose} title={t("organization")}>
      {organization && (
        <>
          <EditOrganizationForm
            key={organization.id}
            organization={organization}
            onSave={(id, organization) => {
              onSave(id, organization);
              onClose();
            }}
          />

          <Divider my="md" />

          <JsonAttributesForm
            attrs={JSON.stringify(organization?.attrs ?? {}, null, 2)}
            onSave={(attrs) => {
              onSave(organization.id, {
                name: organization.name,
                description: organization.description ?? null,
                attrs,
              });
              onClose();
            }}
          />
        </>
      )}
    </EntityDrawer>
  );
};

export default EditOrganization;
