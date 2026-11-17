import React from "react";
import {
  ActionIcon,
  useComputedColorScheme,
  useMantineColorScheme,
} from "@mantine/core";
import { LuMoon, LuSun } from "react-icons/lu";

interface Props {
  className?: string;
  label?: string;
}

const ColorSchemeToggle: React.FC<Props> = ({
  className,
  label = "Toggle color scheme",
}) => {
  const { setColorScheme } = useMantineColorScheme();
  const scheme = useComputedColorScheme("light");

  return (
    <ActionIcon
      size="lg"
      color="gray"
      aria-label={label}
      className={className}
      onClick={() => setColorScheme(scheme === "light" ? "dark" : "light")}
    >
      {scheme === "dark" ? <LuSun size={17} /> : <LuMoon size={17} />}
    </ActionIcon>
  );
};

export default ColorSchemeToggle;
