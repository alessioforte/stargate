import React from "react";
import {
  ActionIcon,
  Box,
  useComputedColorScheme,
  useMantineColorScheme,
} from "@mantine/core";
import { CiLight, CiDark } from "react-icons/ci";

interface Props {
  color?: string;
  className?: string;
  size?: string;
}

// Both icons are rendered and CSS shows the one for the active scheme, so
// the markup does not depend on browser state (the login page is
// prerendered and hydrated).
const ColorSchemeToggle: React.FC<Props> = ({
  color = "gray",
  className,
  size = "xl",
}) => {
  const { setColorScheme } = useMantineColorScheme();
  const computedColorScheme = useComputedColorScheme("light", {
    getInitialValueInEffect: true,
  });
  return (
    <ActionIcon
      size={size}
      aria-label="Theme toggle"
      className={className}
      variant="transparent"
      color={color}
      onClick={() => {
        setColorScheme(computedColorScheme === "light" ? "dark" : "light");
      }}
    >
      <Box component="span" darkHidden>
        <CiLight />
      </Box>
      <Box component="span" lightHidden>
        <CiDark />
      </Box>
    </ActionIcon>
  );
};

export default ColorSchemeToggle;
