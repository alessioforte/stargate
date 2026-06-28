import { createContext } from "react";

export interface MultiDrawerContextProps {
  opened: boolean;
  wideOpened: boolean;
  top: number;
  onClose: () => void;
  onWideClose: () => void;
}

export const MultiDrawerContext = createContext<MultiDrawerContextProps | null>(
  null,
);
export const MultiDrawerProvider = MultiDrawerContext.Provider;
