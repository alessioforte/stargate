import { showNotification as show } from "@mantine/notifications";
import { IoIosClose } from "react-icons/io";
import { IoCheckmark } from "react-icons/io5";
import { CiCircleInfo, CiWarning } from "react-icons/ci";
import classes from "./Notification.module.css";

type NotificationType = "success" | "error" | "info" | "warning";

const NotificationTypes: Record<NotificationType, any> = {
  error: {
    icon: <IoIosClose />,
    color: "red",
    className: classes.error,
  },
  success: {
    icon: <IoCheckmark />,
    color: "teal",
    className: classes.success,
  },
  info: {
    icon: <CiCircleInfo />,
    color: "cyan",
    className: classes.info,
  },
  warning: {
    icon: <CiWarning />,
    color: "yellow",
    className: classes.warning,
  },
};

interface Props {
  type?: NotificationType;
  title?: string;
  message?: string;
  autoClose?: number | boolean;
}

const defaultAutoCloses = {
  success: 5000,
  error: 6000,
  info: 5000,
  warning: 5000,
};

const showNotification = ({
  title,
  message,
  type = "info",
  autoClose,
}: Props) => {
  const finalAutoClose = autoClose ?? defaultAutoCloses[type];

  return show({
    title,
    message,
    withBorder: true,
    autoClose: finalAutoClose,
    ...NotificationTypes[type],
  });
};

export default showNotification;
