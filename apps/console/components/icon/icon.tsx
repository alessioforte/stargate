import React from "react";
import icons, { type IconNames } from "./icons";
import { Box, rem, useMantineTheme } from "@mantine/core";
import classes from "./icon.module.css";

interface Props extends React.ComponentPropsWithoutRef<"svg"> {
  name: IconNames;
  color?: string;
  size?: number | string;
  className?: string;
  strokeWidth?: number;
}

const Icon = ({ name, color, size = 18, className, strokeWidth }: Props) => {
  const { colors } = useMantineTheme();
  const icon = icons[name];
  const fill = colors[color ?? ""]?.[6] ?? color;
  const style = color ? { fill } : {};

  return (
    icon && (
      <Box
        x="0px"
        y="0px"
        component="svg"
        height={`${size}px`}
        viewBox={`0 0 ${icon.width} 24`}
        className={className ?? classes.icon}
        style={{ width: rem(size), height: rem(size), ...style }}
      >
        {icon?.group ? (
          icon?.group({ color: fill, strokeWidth })
        ) : (
          <path d={icon.d} />
        )}
      </Box>
    )
  );
};

export default Icon;
