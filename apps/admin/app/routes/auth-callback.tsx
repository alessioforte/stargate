import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Alert, Button, Center, Stack, Text } from "@mantine/core";
import {
  clearOAuthRequest,
  clearTokens,
  exchangeAuthorizationCode,
  getOAuthRequest,
  redirectToHostedLogin,
  storeTokens,
} from "@/lib/oauth";
import services from "@/services";

type CallbackState =
  | { status: "loading"; message: string }
  | { status: "error"; message: string };

export default function AuthCallbackPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [state, setState] = useState<CallbackState>({
    status: "loading",
    message: "Completing sign in",
  });

  useEffect(() => {
    let cancelled = false;

    async function completeCallback() {
      const request = getOAuthRequest();
      const returnedState = searchParams.get("state");

      if (!request || !returnedState || request.state !== returnedState) {
        clearTokens();
        clearOAuthRequest();
        setState({
          status: "error",
          message: "Invalid OAuth state. Start sign in again.",
        });
        return;
      }

      const error = searchParams.get("error");
      if (error) {
        if (error === "login_required") {
          setState({
            status: "loading",
            message: "Redirecting to sign in",
          });
          await redirectToHostedLogin(request.returnPath);
          return;
        }

        clearOAuthRequest();
        setState({
          status: "error",
          message:
            searchParams.get("error_description") ??
            `OAuth authorization failed: ${error}`,
        });
        return;
      }

      const code = searchParams.get("code");
      if (!code) {
        setState({
          status: "error",
          message: "Missing OAuth authorization code.",
        });
        return;
      }

      const tokens = await exchangeAuthorizationCode(
        code,
        request.codeVerifier,
      );
      if (cancelled) return;

      storeTokens(tokens);
      services.storeTokens(tokens);
      clearOAuthRequest();
      navigate(request.returnPath || "/", { replace: true });
    }

    completeCallback().catch((error: unknown) => {
      if (cancelled) return;
      clearTokens();
      setState({
        status: "error",
        message:
          error instanceof Error ? error.message : "OAuth callback failed.",
      });
    });

    return () => {
      cancelled = true;
    };
  }, [navigate, searchParams]);

  if (state.status === "error") {
    return (
      <Center mih="100vh">
        <Stack w={380}>
          <Alert color="red" title="Authentication error">
            {state.message}
          </Alert>
          <Button onClick={() => redirectToHostedLogin("/")}>Sign in</Button>
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
