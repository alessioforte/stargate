import { Alert, Button, Center, Stack } from "@mantine/core";
import useStore from "@/store";

export default function ErrorPage() {
  const message =
    useStore((state) => state.adminError) ?? "Unable to open Stargate Admin.";
  const returnPath = useStore((state) => state.adminReturnPath);
  const signIn = useStore((state) => state.signIn);

  return (
    <Center mih="100vh">
      <Stack w={380}>
        <Alert color="red" title="Authentication error">
          {message}
        </Alert>
        <Button onClick={() => signIn(returnPath)}>Sign in</Button>
      </Stack>
    </Center>
  );
}
