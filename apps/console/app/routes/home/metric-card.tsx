import { Card, Group, Skeleton, Stack, Text, ThemeIcon } from "@mantine/core";

interface MetricCardProps {
  detail: string;
  icon: React.ReactNode;
  label: string;
  loading: boolean;
  value?: number;
}

function MetricCard({ detail, icon, label, loading, value }: MetricCardProps) {
  return (
    <Card withBorder radius="md" p={8}>
      <Group justify="space-between" align="center" wrap="nowrap">
        <Text c="dimmed" size="xs" fw={600} tt="uppercase">
          {label}
        </Text>
        <ThemeIcon variant="transparent" size="md">
          {icon}
        </ThemeIcon>
      </Group>
      <Stack gap={3}>
        {loading ? (
          <Skeleton h={30} w={70} mt={3} />
        ) : (
          <Text fw={700} size="xl">
            {value?.toLocaleString() ?? "—"}
          </Text>
        )}
        <Text c="dimmed" size="xs">
          {detail}
        </Text>
      </Stack>
    </Card>
  );
}

export default MetricCard;
