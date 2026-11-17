import { Badge } from "@mantine/core";
import classes from "./home.module.css";

function StatusRow({
  color,
  detail,
  label,
  status,
}: {
  /** Badge color; by default teal when `status` is `healthy`, else red. */
  color?: string;
  detail?: string;
  label: string;
  status: string;
}) {
  return (
    <div className={classes.row}>
      <div className={classes.rowLabel}>
        {label}
        {detail && <div className={classes.rowDetail}>{detail}</div>}
      </div>
      <Badge color={color ?? (status === "healthy" ? "teal" : "red")}>
        {status}
      </Badge>
    </div>
  );
}

export default StatusRow;
