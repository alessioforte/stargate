import { Box, Stack, Text } from "@mantine/core";
import useStore from "@/store";

export default function AdminHomePage() {
  const adminStatus = useStore((state) => state.adminStatus);

  return (
    <Box p="xl">
      <Stack gap="xs">
        <Text c="dimmed" size="sm">
          Status: {adminStatus?.status ?? "unknown"}
        </Text>
        <Text c="dimmed" size="sm">
          API: {adminStatus?.apiName ?? "Stargate"}
        </Text>
        <Text c="dimmed" size="sm">
          Version: {adminStatus?.version ?? "unknown"}
        </Text>
        <Text c="dimmed" size="sm">
          Database: {adminStatus?.databaseStatus ?? "unknown"}
        </Text>
        <Text c="dimmed" size="sm">
          Store: {adminStatus?.redisStatus ?? "unknown"}
        </Text>
      </Stack>
    </Box>
  );
}
