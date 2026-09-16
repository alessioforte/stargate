import { useEffect, useRef } from "react";

const DEFAULT_POLLING_INTERVAL = 5000;

type PollingCallback = () => void | Promise<void>;

export interface UsePollingOptions {
  enabled?: boolean;
  immediate?: boolean;
  interval?: number;
  onError?: (error: unknown) => void;
}

const usePolling = (
  pollingFn: PollingCallback,
  {
    enabled = true,
    immediate = false,
    interval = DEFAULT_POLLING_INTERVAL,
    onError,
  }: UsePollingOptions = {},
) => {
  const pollingFnRef = useRef(pollingFn);
  const onErrorRef = useRef(onError);

  useEffect(() => {
    pollingFnRef.current = pollingFn;
  }, [pollingFn]);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    if (!enabled || interval <= 0 || !Number.isFinite(interval)) return;

    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;

    const scheduleNextPoll = () => {
      if (cancelled) return;

      timeout = setTimeout(() => {
        void poll();
      }, interval);
    };

    const poll = async () => {
      try {
        await pollingFnRef.current();
      } catch (error) {
        if (onErrorRef.current) {
          onErrorRef.current(error);
        } else {
          console.error(error);
        }
      } finally {
        scheduleNextPoll();
      }
    };

    if (immediate) {
      void poll();
    } else {
      scheduleNextPoll();
    }

    return () => {
      cancelled = true;

      if (timeout) {
        clearTimeout(timeout);
      }
    };
  }, [enabled, immediate, interval]);
};

export default usePolling;
