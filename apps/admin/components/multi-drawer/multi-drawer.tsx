"use client";

import React, { useContext } from "react";
import { createPortal } from "react-dom";
import {
  Box,
  Group,
  CloseButton,
  Transition,
  Title,
  ScrollArea,
} from "@mantine/core";
import {
  MultiDrawerProvider,
  MultiDrawerContext,
} from "./multi-drawer.context";
import classes from "./multi-drawer.module.css";

interface Props {
  opened: boolean;
  wideOpened: boolean;
  top?: number;
  onClose: () => void;
  onWideClose: () => void;
  children: React.ReactNode;
  overlayProps?: React.HTMLAttributes<HTMLDivElement>;
}

interface CompoundComponents {
  MainPanel: React.FC<PanelProps>;
  WidePanel: React.FC<PanelProps>;
}

const MultiDrawer: React.FC<Props> & CompoundComponents = ({
  opened,
  wideOpened,
  top = 0,
  children,
  overlayProps,
  onClose,
  onWideClose,
}) => {
  let _main: any, _wide: any;

  React.Children.forEach(children, (child: any) => {
    if (child?.type === MainPanel) {
      _main = child;
    }

    if (child?.type === WidePanel) {
      _wide = child;
    }
  });

  return (
    <MultiDrawerProvider
      value={{
        opened,
        wideOpened,
        top,
        onClose,
        onWideClose,
      }}
    >
      {createPortal(
        <>
          <Transition mounted={opened} transition="fade">
            {(style) => (
              <Box
                style={{
                  ...style,
                  ...overlayProps?.style,
                  zIndex: 9,
                }}
                className={classes.overlay}
                onClick={() => {
                  onClose();
                  onWideClose();
                }}
                {...overlayProps}
              />
            )}
          </Transition>
          {_wide}
          {_main}
        </>,
        document.body,
      )}
    </MultiDrawerProvider>
  );
};

interface PanelProps {
  children?: React.ReactNode;
  title?: string;
  header?: React.ReactNode;
  style?: React.CSSProperties;
}

const MainPanel: React.FC<PanelProps> = ({
  title,
  children,
  header,
  style,
}) => {
  const { opened, top, onClose, onWideClose } = useContext(MultiDrawerContext)!;
  return (
    <Box
      style={{
        top,
        zIndex: 9,
        width: 440,
        height: `calc(100% - ${top}px)`,
        position: "absolute",
        right: opened ? 0 : -440,
        ...style,
      }}
      className={classes.panel}
    >
      {header ?? (
        <Group p={12} justify="space-between">
          {title ? <Title order={4}>{title}</Title> : <div />}
          <CloseButton
            onClick={() => {
              onClose();
              onWideClose();
            }}
          />
        </Group>
      )}
      <Box component={ScrollArea} h="calc(100vh - 52px)">
        {children}
      </Box>
    </Box>
  );
};

const WidePanel: React.FC<PanelProps> = ({
  title,
  children,
  header,
  style,
}) => {
  const { opened, wideOpened, top, onWideClose } =
    useContext(MultiDrawerContext)!;
  const nestedLeft = opened ? "calc(100vw - 440px)" : "100vw";
  return (
    <Box
      style={{
        top,
        zIndex: 9,
        height: `calc(100% - ${top}px)`,
        position: "absolute",
        width: "calc(100vw - 440px)",
        left: wideOpened ? 0 : nestedLeft,
        visibility: opened ? "visible" : "hidden",
        ...style,
      }}
      className={classes.nested}
    >
      {header ?? (
        <Group p={12} justify="space-between">
          {title ? <Title order={4}>{title}</Title> : <div />}
          <CloseButton onClick={onWideClose} />
        </Group>
      )}
      {children}
    </Box>
  );
};

MultiDrawer.MainPanel = MainPanel;
MultiDrawer.WidePanel = WidePanel;

export default MultiDrawer;
