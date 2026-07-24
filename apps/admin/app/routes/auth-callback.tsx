import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Alert, Button, Center, Stack, Text } from "@mantine/core";
import useStore from "@/store";

type CallbackState =
  | { status: "loading"; message: string }
  | { status: "error"; message: string };

export default function AuthCallbackPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const completeOAuthCallback = useStore(
    (store) => store.completeOAuthCallback,
  );
  const signIn = useStore((store) => store.signIn);
  const [state, setState] = useState<CallbackState>({
    status: "loading",
    message: "Completing sign in",
  });

  async function restartSignIn() {
    setState({
      status: "loading",
      message: "Redirecting to sign in",
    });

    const result = await signIn("/");
    if (result.status === "error") {
      setState({
        status: "error",
        message: result.message,
      });
    }
  }

  useEffect(() => {
    let cancelled = false;

    void completeOAuthCallback({
      code: searchParams.get("code"),
      error: searchParams.get("error"),
      errorDescription: searchParams.get("error_description"),
      state: searchParams.get("state"),
    }).then((result) => {
      if (cancelled) return;

      if (result.status === "success") {
        navigate(result.returnPath, { replace: true });
      } else if (result.status === "redirecting") {
        setState({
          status: "loading",
          message: "Redirecting to sign in",
        });
      } else {
        setState({
          status: "error",
          message: result.message,
        });
      }
    });

    return () => {
      cancelled = true;
    };
  }, [completeOAuthCallback, navigate, searchParams]);

  if (state.status === "error") {
    return (
      <Center mih="100vh">
        <Stack w={380}>
          <Alert color="red" title="Authentication error">
            {state.message}
          </Alert>
          <Button onClick={() => void restartSignIn()}>Sign in</Button>
        </Stack>
      </Center>
    );
  }

  return (
    <Center mih="100vh">
      <Text c="dimmed">{state.message}</Text>
    </Center>
  );
}
