import { Center } from "@mantine/core";
import Icon from "../icon/icon";
import type * as CSS from "csstype";

interface NoDataProps {
  icon?: string | React.ReactNode;
  size?: number;
  style?: CSS.Properties;
}

const Empty: React.FC<NoDataProps> = ({
  icon = "sauropod",
  size = 120,
  style = {},
}) => {
  return (
    <Center
      style={{ minHeight: "200px", ...style }}
      data-testid="NoDataComponent"
    >
      {typeof icon === "string" ? (
        <Icon name={icon} color="primary" size={size} />
      ) : (
        icon
      )}
    </Center>
  );
};

export default Empty;
