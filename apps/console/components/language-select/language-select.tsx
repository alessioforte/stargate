import React, { forwardRef } from "react";
import { ActionIcon, Box, Group, Text, Menu, Tooltip } from "@mantine/core";
import { CiGlobe } from "react-icons/ci";

type FloatingPlacement = "end" | "start";
type FloatingSide = "top" | "right" | "bottom" | "left";
type FloatingPosition = FloatingSide | `${FloatingSide}-${FloatingPlacement}`;

interface ItemProps extends React.ComponentPropsWithoutRef<"div"> {
  value: string;
  label: string;
}

// eslint-disable-next-line react/display-name
const Item = forwardRef<HTMLDivElement, ItemProps>(
  ({ value, label, ...others }: ItemProps, ref) => (
    <div ref={ref} {...others}>
      <Group>
        <Box
          style={{
            width: 20,
            height: 20,
            borderRadius: "50%",
            backgroundImage: `url(https://flagcdn.com/${
              value === "en" ? "gb" : value
            }.svg)`,
            backgroundRepeat: "no-repeat, repeat",
            backgroundPosition: "center",
            backgroundSize: "cover",
          }}
        />

        <div>
          <Text size="sm">{label}</Text>
        </div>
      </Group>
    </div>
  ),
);

interface Props {
  value?: string;
  options: { label: string; value: string; href: string }[];
  onChange?: (value: string) => void;
  tooltipLabel?: string;
  dropdownPosition?: FloatingPosition;
}

const LanguageSelect: React.FC<Props> = ({
  value,
  options,
  onChange = () => null,
  tooltipLabel = "Select language",
  dropdownPosition,
}) => {
  const lang = value === "en" ? "gb" : value;

  const handleOnChange = (val: string | null) => {
    if (val !== null) {
      onChange(val);
    }
    // if (option) {
    //   window.location.href = option.href || window.location.href;
    // }
  };

  return (
    <Menu position={dropdownPosition} withinPortal>
      <Menu.Target>
        <Tooltip
          position="right"
          label={tooltipLabel}
          openDelay={300}
          offset={20}
        >
          <ActionIcon
            size="sm"
            variant="transparent"
            style={{ "&:hover": { background: "transparent" } }}
          >
            {lang ? (
              <Box
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: "50%",
                  backgroundImage: `url(https://flagcdn.com/${lang}.svg)`,
                  backgroundRepeat: "no-repeat, repeat",
                  backgroundPosition: "center",
                  backgroundSize: "cover",
                }}
              />
            ) : (
              <CiGlobe />
            )}
          </ActionIcon>
        </Tooltip>
      </Menu.Target>
      <Menu.Dropdown>
        {options.map((option) => (
          <Menu.Item
            key={option.value}
            onClick={() => handleOnChange(option.value)}
          >
            <Item value={option.value} label={option.label} />
          </Menu.Item>
        ))}
      </Menu.Dropdown>
    </Menu>
  );
};

export default LanguageSelect;
