import { useRef, useEffect } from "react";

const useDebounce = <T extends (...args: any[]) => void>(
  callback: T,
  delay: number,
) => {
  const timeout = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const debounce = (...args: Parameters<T>) => {
    window.clearTimeout(timeout.current);
    timeout.current = window.setTimeout(() => {
      callback(...args);
    }, delay);
  };

  const cancel = () => {
    window.clearTimeout(timeout.current);
  };

  useEffect(() => {
    return () => cancel();
  }, []);

  return { debounce, cancel };
};

export default useDebounce;
