import React from "react";
import { ActionIcon, useMantineColorScheme } from "@mantine/core";
import { CiLight, CiDark } from "react-icons/ci";

type ColorScheme = "light" | "dark" | "system";

interface Props {
  theme: ColorScheme;
  color?: string;
  className?: string;
  size?: string;
  onClick: (theme: ColorScheme) => void;
}

const ColorSchemeToggle: React.FC<Props> = ({
  theme,
  color = "gray",
  className,
  size = "xl",
  onClick,
}) => {
  const { setColorScheme } = useMantineColorScheme();

  return (
    <ActionIcon
      size={size}
      aria-label="Theme toggle"
      className={className}
      variant="transparent"
      color={color}
      onClick={() => {
        const colorScheme = theme === "light" ? "dark" : "light";
        setColorScheme(colorScheme);
        onClick(colorScheme);
      }}
    >
      {theme === "dark" ? <CiDark /> : <CiLight />}
    </ActionIcon>
  );
};

export default ColorSchemeToggle;
