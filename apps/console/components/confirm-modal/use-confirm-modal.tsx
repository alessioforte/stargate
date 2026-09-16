import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Button, Group, Modal, Stack, Text } from "@mantine/core";
import { useTranslations } from "@/i18n";

interface ConfirmOptions {
  cancelLabel?: string;
  color?: string;
  confirmLabel?: string;
  message?: ReactNode;
  title: string;
}

function renderMessage(message: ReactNode) {
  if (!message) return null;
  if (typeof message === "string") {
    return <Text size="sm">{message}</Text>;
  }

  return message;
}

export default function useConfirmModal() {
  const t = useTranslations();
  const [options, setOptions] = useState<ConfirmOptions | null>(null);
  const resolver = useRef<((confirmed: boolean) => void) | null>(null);

  const close = useCallback((confirmed: boolean) => {
    resolver.current?.(confirmed);
    resolver.current = null;
    setOptions(null);
  }, []);

  const confirm = useCallback((nextOptions: ConfirmOptions) => {
    resolver.current?.(false);

    return new Promise<boolean>((resolve) => {
      resolver.current = resolve;
      setOptions(nextOptions);
    });
  }, []);

  useEffect(() => {
    return () => {
      resolver.current?.(false);
    };
  }, []);

  const confirmModal = (
    <Modal
      centered
      opened={Boolean(options)}
      onClose={() => close(false)}
      title={options?.title}
    >
      <Stack>
        {renderMessage(options?.message)}
        <Group justify="flex-end">
          <Button type="button" variant="default" onClick={() => close(false)}>
            {options?.cancelLabel ?? t("cancel")}
          </Button>
          <Button
            type="button"
            color={options?.color ?? "red"}
            onClick={() => close(true)}
          >
            {options?.confirmLabel ?? t("confirm")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );

  return { confirm, confirmModal };
}
