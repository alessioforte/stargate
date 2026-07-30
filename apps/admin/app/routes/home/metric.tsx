import { Text } from "@mantine/core";

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <Text size="xs" c="dimmed">
        {label}
      </Text>
      <Text size="xl" fw={700}>
        {value.toLocaleString()}
      </Text>
    </div>
  );
}

export default Metric;
