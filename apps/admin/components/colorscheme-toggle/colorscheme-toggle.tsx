import React from "react";
import { ActionIcon, useMantineColorScheme } from "@mantine/core";
import { useColorScheme } from "@mantine/hooks";
import { CiLight, CiDark } from "react-icons/ci";

interface Props {
  color?: string;
  className?: string;
  size?: string;
}

const ColorSchemeToggle: React.FC<Props> = ({
  color = "gray",
  className,
  size = "xl",
}) => {
  const defaultColorScheme = useColorScheme();
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const t = colorScheme === "auto" ? defaultColorScheme : colorScheme;
  return (
    <ActionIcon
      size={size}
      aria-label="Theme toggle"
      className={className}
      variant="transparent"
      color={color}
      onClick={() => {
        const cs = t === "light" ? "dark" : "light";
        setColorScheme(cs);
      }}
    >
      {t === "dark" ? <CiDark /> : <CiLight />}
    </ActionIcon>
  );
};

export default ColorSchemeToggle;
