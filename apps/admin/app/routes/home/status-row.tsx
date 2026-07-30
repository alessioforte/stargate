import { Badge, Group, Stack, Text } from "@mantine/core";

function StatusRow({
  detail,
  label,
  status,
}: {
  detail?: string;
  label: string;
  status: string;
}) {
  return (
    <Group justify="space-between" wrap="nowrap">
      <Stack gap={0}>
        <Text size="sm" fw={500}>
          {label}
        </Text>
        {detail && (
          <Text size="xs" c="dimmed">
            {detail}
          </Text>
        )}
      </Stack>
      <Badge variant="light" color={status === "healthy" ? "teal" : "red"}>
        {status}
      </Badge>
    </Group>
  );
}

export default StatusRow;
