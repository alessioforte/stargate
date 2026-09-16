import type { ComponentProps, ReactNode } from "react";
import { Button, Drawer, Flex, Group, ScrollArea, Title } from "@mantine/core";
import { IoCloseSharp } from "react-icons/io5";

interface Props {
  children: ReactNode;
  opened: boolean;
  title: ReactNode;
  onClose: () => void;
  position?: ComponentProps<typeof Drawer.Root>["position"];
  size?: ComponentProps<typeof Drawer.Root>["size"];
}

const EntityDrawer: React.FC<Props> = ({
  children,
  opened,
  title,
  onClose,
  position = "right",
  size = "md",
}) => {
  return (
    <Drawer.Root
      size={size}
      opened={opened}
      onClose={onClose}
      position={position}
    >
      <Drawer.Overlay />
      <Drawer.Content>
        <Flex direction="column" style={{ height: "100vh" }}>
          <Group p="xs" justify="space-between">
            <Title order={4}>{title}</Title>
            <Group gap="xs">
              <Button
                type="button"
                size="compact-sm"
                onClick={onClose}
                variant="transparent"
              >
                <IoCloseSharp />
              </Button>
            </Group>
          </Group>

          <ScrollArea style={{ flex: 1, overflowY: "auto" }}>
            {children}
          </ScrollArea>
        </Flex>
      </Drawer.Content>
    </Drawer.Root>
  );
};

export default EntityDrawer;
