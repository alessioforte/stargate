import classes from "./home.module.css";

function DetailRow({
  children,
  label,
}: {
  children: React.ReactNode;
  label: React.ReactNode;
}) {
  return (
    <div className={classes.row}>
      <div className={classes.rowLabel}>{label}</div>
      <div className={classes.rowValue}>{children}</div>
    </div>
  );
}

export default DetailRow;
