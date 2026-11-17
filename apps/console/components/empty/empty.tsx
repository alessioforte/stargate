import { Stack, Text, ThemeIcon } from "@mantine/core";
import { LuInbox } from "react-icons/lu";

interface Props {
  description?: React.ReactNode;
  icon?: React.ReactNode;
  title?: React.ReactNode;
}

const Empty: React.FC<Props> = ({ description, icon, title }) => {
  return (
    <Stack align="center" justify="center" gap={6} py={56} px="md">
      <ThemeIcon size={44} radius="xl" variant="light" color="gray" mb={6}>
        {icon ?? <LuInbox size={22} />}
      </ThemeIcon>
      {title && (
        <Text size="sm" fw={600}>
          {title}
        </Text>
      )}
      {description && (
        <Text size="sm" c="dimmed" ta="center" maw={360}>
          {description}
        </Text>
      )}
    </Stack>
  );
};

export default Empty;
