import type { ComponentProps, ReactNode } from "react";
import {
  CloseButton,
  Drawer,
  Flex,
  Group,
  ScrollArea,
  Title,
} from "@mantine/core";

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
          <Group
            px="md"
            h={56}
            justify="space-between"
            wrap="nowrap"
            style={{
              flex: "0 0 auto",
              borderBottom: "1px solid var(--app-border)",
            }}
          >
            <Title order={4}>{title}</Title>
            <CloseButton onClick={onClose} />
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
