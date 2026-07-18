import { Box, Text, Title } from "@mantine/core";

export default function AdminHomePage() {
  return (
    <Box p="xl">
      <Title order={3}>Welcome</Title>
      <Text c="dimmed" size="sm" mt="xs">
        System status is displayed in the footer.
      </Text>
    </Box>
  );
}
