import { Skeleton } from "@mantine/core";
import { Link } from "react-router";
import classes from "./home.module.css";

interface MetricCardProps {
  detail: string;
  icon: React.ReactNode;
  label: string;
  loading: boolean;
  to: string;
  value?: number;
}

function MetricCard({
  detail,
  icon,
  label,
  loading,
  to,
  value,
}: MetricCardProps) {
  return (
    <Link to={to} className={classes.stat}>
      <div className={classes.statHead}>
        {label}
        {icon}
      </div>
      {loading ? (
        <Skeleton h={32} w={72} />
      ) : (
        <div className={classes.statValue}>
          {value?.toLocaleString() ?? "—"}
        </div>
      )}
      <div className={classes.statDetail}>{detail}</div>
    </Link>
  );
}

export default MetricCard;
